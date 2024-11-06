use std::{
    array::from_fn,
    borrow::{Borrow, BorrowMut},
    cell::RefCell,
    iter::zip,
    marker::PhantomData,
};

use ax_circuit_derive::AlignedBorrow;
use ax_stark_backend::interaction::InteractionBuilder;
use axvm_instructions::instruction::Instruction;
use itertools::izip;
use p3_air::BaseAir;
use p3_field::{AbstractField, Field, PrimeField32};

use super::{read_rv32_register, RV32_CELL_BITS, RV32_REGISTER_NUM_LIMBS};
use crate::{
    arch::{
        AdapterAirContext, AdapterRuntimeContext, ExecutionBridge, ExecutionBus, ExecutionState,
        Result, VecHeapTwoReadsAdapterInterface, VmAdapterAir, VmAdapterChip, VmAdapterInterface,
    },
    system::{
        memory::{
            offline_checker::{MemoryBridge, MemoryReadAuxCols, MemoryWriteAuxCols},
            MemoryAddress, MemoryAuxColsFactory, MemoryController, MemoryControllerRef,
            MemoryReadRecord, MemoryWriteRecord,
        },
        program::ProgramBus,
    },
};

/// This adapter reads from 2 pointers and writes to 1 pointer.
/// * The data is read from the heap (address space 2), and the pointers
///   are read from registers (address space 1).
/// * Reads take the form of `BLOCKS_PER_READX` consecutive reads of size
///   `READ_SIZE` from the heap, starting from the addresses in `rs[X]`
/// * NOTE that the two reads can read different numbers of blocks.
/// * Writes take the form of `BLOCKS_PER_WRITE` consecutive writes of
///   size `WRITE_SIZE` to the heap, starting from the address in `rd`.
#[derive(Debug)]
pub struct Rv32VecHeapTwoReadsAdapterChip<
    F: Field,
    const BLOCKS_PER_READ1: usize,
    const BLOCKS_PER_READ2: usize,
    const BLOCKS_PER_WRITE: usize,
    const READ_SIZE: usize,
    const WRITE_SIZE: usize,
> {
    pub air: Rv32VecHeapTwoReadsAdapterAir<
        BLOCKS_PER_READ1,
        BLOCKS_PER_READ2,
        BLOCKS_PER_WRITE,
        READ_SIZE,
        WRITE_SIZE,
    >,
    _marker: PhantomData<F>,
}

impl<
        F: PrimeField32,
        const BLOCKS_PER_READ1: usize,
        const BLOCKS_PER_READ2: usize,
        const BLOCKS_PER_WRITE: usize,
        const READ_SIZE: usize,
        const WRITE_SIZE: usize,
    >
    Rv32VecHeapTwoReadsAdapterChip<
        F,
        BLOCKS_PER_READ1,
        BLOCKS_PER_READ2,
        BLOCKS_PER_WRITE,
        READ_SIZE,
        WRITE_SIZE,
    >
{
    pub fn new(
        execution_bus: ExecutionBus,
        program_bus: ProgramBus,
        memory_controller: MemoryControllerRef<F>,
    ) -> Self {
        let memory_controller = RefCell::borrow(&memory_controller);
        let memory_bridge = memory_controller.memory_bridge();
        let address_bits = memory_controller.mem_config.pointer_max_bits;
        Self {
            air: Rv32VecHeapTwoReadsAdapterAir {
                execution_bridge: ExecutionBridge::new(execution_bus, program_bus),
                memory_bridge,
                address_bits,
            },
            _marker: PhantomData,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Rv32VecHeapTwoReadsReadRecord<
    F: Field,
    const BLOCKS_PER_READ1: usize,
    const BLOCKS_PER_READ2: usize,
    const READ_SIZE: usize,
> {
    /// Read register value from address space e=1
    pub rs1: MemoryReadRecord<F, RV32_REGISTER_NUM_LIMBS>,
    pub rs2: MemoryReadRecord<F, RV32_REGISTER_NUM_LIMBS>,
    /// Read register value from address space d=1
    pub rd: MemoryReadRecord<F, RV32_REGISTER_NUM_LIMBS>,

    pub rd_val: F,

    pub reads1: [MemoryReadRecord<F, READ_SIZE>; BLOCKS_PER_READ1],
    pub reads2: [MemoryReadRecord<F, READ_SIZE>; BLOCKS_PER_READ2],
}

#[derive(Clone, Debug)]
pub struct Rv32VecHeapTwoReadsWriteRecord<
    F: Field,
    const BLOCKS_PER_WRITE: usize,
    const WRITE_SIZE: usize,
> {
    pub from_state: ExecutionState<u32>,

    pub writes: [MemoryWriteRecord<F, WRITE_SIZE>; BLOCKS_PER_WRITE],
}

#[repr(C)]
#[derive(AlignedBorrow)]
pub struct Rv32VecHeapTwoReadsAdapterCols<
    T,
    const BLOCKS_PER_READ1: usize,
    const BLOCKS_PER_READ2: usize,
    const BLOCKS_PER_WRITE: usize,
    const READ_SIZE: usize,
    const WRITE_SIZE: usize,
> {
    pub from_state: ExecutionState<T>,

    pub rs1_ptr: T,
    pub rs2_ptr: T,
    pub rd_ptr: T,

    pub rs1_val: [T; RV32_REGISTER_NUM_LIMBS],
    pub rs2_val: [T; RV32_REGISTER_NUM_LIMBS],
    pub rd_val: [T; RV32_REGISTER_NUM_LIMBS],

    pub rs1_read_aux: MemoryReadAuxCols<T, RV32_REGISTER_NUM_LIMBS>,
    pub rs2_read_aux: MemoryReadAuxCols<T, RV32_REGISTER_NUM_LIMBS>,
    pub rd_read_aux: MemoryReadAuxCols<T, RV32_REGISTER_NUM_LIMBS>,

    pub reads1_aux: [MemoryReadAuxCols<T, READ_SIZE>; BLOCKS_PER_READ1],
    pub reads2_aux: [MemoryReadAuxCols<T, READ_SIZE>; BLOCKS_PER_READ2],
    pub writes_aux: [MemoryWriteAuxCols<T, WRITE_SIZE>; BLOCKS_PER_WRITE],
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, derive_new::new)]
pub struct Rv32VecHeapTwoReadsAdapterAir<
    const BLOCKS_PER_READ1: usize,
    const BLOCKS_PER_READ2: usize,
    const BLOCKS_PER_WRITE: usize,
    const READ_SIZE: usize,
    const WRITE_SIZE: usize,
> {
    pub(super) execution_bridge: ExecutionBridge,
    pub(super) memory_bridge: MemoryBridge,
    /// The max number of bits for an address in memory
    address_bits: usize,
}

impl<
        F: Field,
        const BLOCKS_PER_READ1: usize,
        const BLOCKS_PER_READ2: usize,
        const BLOCKS_PER_WRITE: usize,
        const READ_SIZE: usize,
        const WRITE_SIZE: usize,
    > BaseAir<F>
    for Rv32VecHeapTwoReadsAdapterAir<
        BLOCKS_PER_READ1,
        BLOCKS_PER_READ2,
        BLOCKS_PER_WRITE,
        READ_SIZE,
        WRITE_SIZE,
    >
{
    fn width(&self) -> usize {
        Rv32VecHeapTwoReadsAdapterCols::<
            F,
            BLOCKS_PER_READ1,
            BLOCKS_PER_READ2,
            BLOCKS_PER_WRITE,
            READ_SIZE,
            WRITE_SIZE,
        >::width()
    }
}

impl<
        AB: InteractionBuilder,
        const BLOCKS_PER_READ1: usize,
        const BLOCKS_PER_READ2: usize,
        const BLOCKS_PER_WRITE: usize,
        const READ_SIZE: usize,
        const WRITE_SIZE: usize,
    > VmAdapterAir<AB>
    for Rv32VecHeapTwoReadsAdapterAir<
        BLOCKS_PER_READ1,
        BLOCKS_PER_READ2,
        BLOCKS_PER_WRITE,
        READ_SIZE,
        WRITE_SIZE,
    >
{
    type Interface = VecHeapTwoReadsAdapterInterface<
        AB::Expr,
        BLOCKS_PER_READ1,
        BLOCKS_PER_READ2,
        BLOCKS_PER_WRITE,
        READ_SIZE,
        WRITE_SIZE,
    >;

    fn eval(
        &self,
        builder: &mut AB,
        local: &[AB::Var],
        ctx: AdapterAirContext<AB::Expr, Self::Interface>,
    ) {
        let cols: &Rv32VecHeapTwoReadsAdapterCols<
            _,
            BLOCKS_PER_READ1,
            BLOCKS_PER_READ2,
            BLOCKS_PER_WRITE,
            READ_SIZE,
            WRITE_SIZE,
        > = local.borrow();
        let timestamp = cols.from_state.timestamp;
        let mut timestamp_delta: usize = 0;
        let mut timestamp_pp = || {
            timestamp_delta += 1;
            timestamp + AB::F::from_canonical_usize(timestamp_delta - 1)
        };

        let ptrs = [cols.rs1_ptr, cols.rs2_ptr, cols.rd_ptr];
        let vals = [cols.rs1_val, cols.rs2_val, cols.rd_val];
        let auxs = [&cols.rs1_read_aux, &cols.rs2_read_aux, &cols.rd_read_aux];

        // Read register values for rs1, rs2, rd
        for (ptr, val, aux) in izip!(ptrs, vals, auxs) {
            self.memory_bridge
                .read(
                    MemoryAddress::new(AB::Expr::one(), ptr),
                    val,
                    timestamp_pp(),
                    aux,
                )
                .eval(builder, ctx.instruction.is_valid.clone());
        }

        // Compose the u32 register value into single field element, with
        // a range check on the highest limb.
        let register_to_field = |r: [AB::Var; RV32_REGISTER_NUM_LIMBS]| {
            r.into_iter()
                .enumerate()
                .fold(AB::Expr::zero(), |acc, (i, limb)| {
                    acc + limb * AB::Expr::from_canonical_usize(1 << (i * RV32_CELL_BITS))
                })
        };
        let rd_val_f = register_to_field(cols.rd_val);
        let rs1_val_f = register_to_field(cols.rs1_val);
        let rs2_val_f = register_to_field(cols.rs2_val);

        let e = AB::F::from_canonical_usize(2);
        // Reads from heap
        for (i, (read, aux)) in zip(ctx.reads.0, &cols.reads1_aux).enumerate() {
            self.memory_bridge
                .read(
                    MemoryAddress::new(
                        e,
                        rs1_val_f.clone() + AB::Expr::from_canonical_usize(i * READ_SIZE),
                    ),
                    read,
                    timestamp_pp(),
                    aux,
                )
                .eval(builder, ctx.instruction.is_valid.clone());
        }
        for (i, (read, aux)) in zip(ctx.reads.1, &cols.reads2_aux).enumerate() {
            self.memory_bridge
                .read(
                    MemoryAddress::new(
                        e,
                        rs2_val_f.clone() + AB::Expr::from_canonical_usize(i * READ_SIZE),
                    ),
                    read,
                    timestamp_pp(),
                    aux,
                )
                .eval(builder, ctx.instruction.is_valid.clone());
        }

        // Writes to heap
        for (i, (write, aux)) in zip(ctx.writes, &cols.writes_aux).enumerate() {
            self.memory_bridge
                .write(
                    MemoryAddress::new(
                        e,
                        rd_val_f.clone() + AB::Expr::from_canonical_usize(i * WRITE_SIZE),
                    ),
                    write,
                    timestamp_pp(),
                    aux,
                )
                .eval(builder, ctx.instruction.is_valid.clone());
        }

        self.execution_bridge
            .execute_and_increment_or_set_pc(
                ctx.instruction.opcode,
                [
                    cols.rd_ptr.into(),
                    cols.rs1_ptr.into(),
                    cols.rs2_ptr.into(),
                    AB::Expr::one(),
                    e.into(),
                ],
                cols.from_state,
                AB::F::from_canonical_usize(timestamp_delta),
                (4, ctx.to_pc),
            )
            .eval(builder, ctx.instruction.is_valid.clone());
    }

    fn get_from_pc(&self, local: &[AB::Var]) -> AB::Var {
        let cols: &Rv32VecHeapTwoReadsAdapterCols<
            _,
            BLOCKS_PER_READ1,
            BLOCKS_PER_READ2,
            BLOCKS_PER_WRITE,
            READ_SIZE,
            WRITE_SIZE,
        > = local.borrow();
        cols.from_state.pc
    }
}

impl<
        F: PrimeField32,
        const BLOCKS_PER_READ1: usize,
        const BLOCKS_PER_READ2: usize,
        const BLOCKS_PER_WRITE: usize,
        const READ_SIZE: usize,
        const WRITE_SIZE: usize,
    > VmAdapterChip<F>
    for Rv32VecHeapTwoReadsAdapterChip<
        F,
        BLOCKS_PER_READ1,
        BLOCKS_PER_READ2,
        BLOCKS_PER_WRITE,
        READ_SIZE,
        WRITE_SIZE,
    >
{
    type ReadRecord =
        Rv32VecHeapTwoReadsReadRecord<F, BLOCKS_PER_READ1, BLOCKS_PER_READ2, READ_SIZE>;
    type WriteRecord = Rv32VecHeapTwoReadsWriteRecord<F, BLOCKS_PER_WRITE, WRITE_SIZE>;
    type Air = Rv32VecHeapTwoReadsAdapterAir<
        BLOCKS_PER_READ1,
        BLOCKS_PER_READ2,
        BLOCKS_PER_WRITE,
        READ_SIZE,
        WRITE_SIZE,
    >;
    type Interface = VecHeapTwoReadsAdapterInterface<
        F,
        BLOCKS_PER_READ1,
        BLOCKS_PER_READ2,
        BLOCKS_PER_WRITE,
        READ_SIZE,
        WRITE_SIZE,
    >;

    fn preprocess(
        &mut self,
        memory: &mut MemoryController<F>,
        instruction: &Instruction<F>,
    ) -> Result<(
        <Self::Interface as VmAdapterInterface<F>>::Reads,
        Self::ReadRecord,
    )> {
        let Instruction { a, b, c, d, e, .. } = *instruction;

        debug_assert_eq!(d.as_canonical_u32(), 1);
        debug_assert_eq!(e.as_canonical_u32(), 2);

        let (rs1_record, rs1_val) = read_rv32_register(memory, d, b);
        let (rs2_record, rs2_val) = read_rv32_register(memory, d, c);
        let (rd_record, rd_val) = read_rv32_register(memory, d, a);

        // TODO: assert address has < 2^address_bits
        let read1_records = from_fn(|i| {
            memory.read::<READ_SIZE>(e, F::from_canonical_u32(rs1_val + (i * READ_SIZE) as u32))
        });
        let read1_data = read1_records.map(|r| r.data);
        let read2_records = from_fn(|i| {
            memory.read::<READ_SIZE>(e, F::from_canonical_u32(rs2_val + (i * READ_SIZE) as u32))
        });
        let read2_data = read2_records.map(|r| r.data);

        let record = Rv32VecHeapTwoReadsReadRecord {
            rs1: rs1_record,
            rs2: rs2_record,
            rd: rd_record,
            rd_val: F::from_canonical_u32(rd_val),
            reads1: read1_records,
            reads2: read2_records,
        };

        Ok(((read1_data, read2_data), record))
    }

    fn postprocess(
        &mut self,
        memory: &mut MemoryController<F>,
        instruction: &Instruction<F>,
        from_state: ExecutionState<u32>,
        output: AdapterRuntimeContext<F, Self::Interface>,
        read_record: &Self::ReadRecord,
    ) -> Result<(ExecutionState<u32>, Self::WriteRecord)> {
        let e = instruction.e;
        let mut i = 0;
        let writes = output.writes.map(|write| {
            let record = memory.write(
                e,
                read_record.rd_val + F::from_canonical_u32((i * WRITE_SIZE) as u32),
                write,
            );
            i += 1;
            record
        });

        Ok((
            ExecutionState {
                pc: from_state.pc + 4,
                timestamp: memory.timestamp(),
            },
            Self::WriteRecord { from_state, writes },
        ))
    }

    fn generate_trace_row(
        &self,
        row_slice: &mut [F],
        read_record: Self::ReadRecord,
        write_record: Self::WriteRecord,
        aux_cols_factory: &MemoryAuxColsFactory<F>,
    ) {
        vec_heap_two_reads_generate_trace_row_impl(
            row_slice,
            &read_record,
            &write_record,
            aux_cols_factory,
        )
    }

    fn air(&self) -> &Self::Air {
        &self.air
    }
}

pub(super) fn vec_heap_two_reads_generate_trace_row_impl<
    F: PrimeField32,
    const BLOCKS_PER_READ1: usize,
    const BLOCKS_PER_READ2: usize,
    const BLOCKS_PER_WRITE: usize,
    const READ_SIZE: usize,
    const WRITE_SIZE: usize,
>(
    row_slice: &mut [F],
    read_record: &Rv32VecHeapTwoReadsReadRecord<F, BLOCKS_PER_READ1, BLOCKS_PER_READ2, READ_SIZE>,
    write_record: &Rv32VecHeapTwoReadsWriteRecord<F, BLOCKS_PER_WRITE, WRITE_SIZE>,
    aux_cols_factory: &MemoryAuxColsFactory<F>,
) {
    let row_slice: &mut Rv32VecHeapTwoReadsAdapterCols<
        F,
        BLOCKS_PER_READ1,
        BLOCKS_PER_READ2,
        BLOCKS_PER_WRITE,
        READ_SIZE,
        WRITE_SIZE,
    > = row_slice.borrow_mut();
    row_slice.from_state = write_record.from_state.map(F::from_canonical_u32);

    row_slice.rd_ptr = read_record.rd.pointer;
    row_slice.rs1_ptr = read_record.rs1.pointer;
    row_slice.rs2_ptr = read_record.rs2.pointer;

    row_slice.rd_val = read_record.rd.data;
    row_slice.rs1_val = read_record.rs1.data;
    row_slice.rs2_val = read_record.rs2.data;

    row_slice.rs1_read_aux = aux_cols_factory.make_read_aux_cols(read_record.rs1);
    row_slice.rs2_read_aux = aux_cols_factory.make_read_aux_cols(read_record.rs2);
    row_slice.rd_read_aux = aux_cols_factory.make_read_aux_cols(read_record.rd);
    row_slice.reads1_aux = read_record
        .reads1
        .map(|r| aux_cols_factory.make_read_aux_cols(r));
    row_slice.reads2_aux = read_record
        .reads2
        .map(|r| aux_cols_factory.make_read_aux_cols(r));
    row_slice.writes_aux = write_record
        .writes
        .map(|w| aux_cols_factory.make_write_aux_cols(w));
}