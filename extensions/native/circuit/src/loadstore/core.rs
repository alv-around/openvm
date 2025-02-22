use std::{
    array,
    borrow::{Borrow, BorrowMut},
    sync::{Arc, Mutex, OnceLock},
};

use openvm_circuit::arch::{
    instructions::UsizeOpcode, AdapterAirContext, AdapterRuntimeContext, ExecutionError, Result,
    Streams, VmAdapterInterface, VmCoreAir, VmCoreChip,
};
use openvm_circuit_primitives_derive::AlignedBorrow;
use openvm_instructions::instruction::Instruction;
use openvm_native_compiler::NativeLoadStoreOpcode;
use openvm_stark_backend::{
    interaction::InteractionBuilder,
    p3_air::BaseAir,
    p3_field::{Field, FieldAlgebra, PrimeField32},
    rap::BaseAirWithPublicValues,
};
use strum::IntoEnumIterator;

use super::super::adapters::loadstore_native_adapter::NativeLoadStoreInstruction;

#[repr(C)]
#[derive(AlignedBorrow)]
pub struct NativeLoadStoreCoreCols<T, const NUM_CELLS: usize> {
    pub is_loadw: T,
    pub is_storew: T,
    pub is_loadw2: T,
    pub is_storew2: T,
    pub is_shintw: T,

    pub pointer_reads: [T; 2],
    pub data_read: T,
    pub data_write: [T; NUM_CELLS],
}

#[derive(Clone, Debug)]
pub struct NativeLoadStoreCoreRecord<F, const NUM_CELLS: usize> {
    pub opcode: NativeLoadStoreOpcode,

    pub pointer_reads: [F; 2],
    pub data_read: F,
    pub data_write: [F; NUM_CELLS],
}

#[derive(Clone, Debug)]
pub struct NativeLoadStoreCoreAir<const NUM_CELLS: usize> {
    pub offset: usize,
}

impl<F: Field, const NUM_CELLS: usize> BaseAir<F> for NativeLoadStoreCoreAir<NUM_CELLS> {
    fn width(&self) -> usize {
        NativeLoadStoreCoreCols::<F, NUM_CELLS>::width()
    }
}

impl<F: Field, const NUM_CELLS: usize> BaseAirWithPublicValues<F>
    for NativeLoadStoreCoreAir<NUM_CELLS>
{
}

impl<AB, I, const NUM_CELLS: usize> VmCoreAir<AB, I> for NativeLoadStoreCoreAir<NUM_CELLS>
where
    AB: InteractionBuilder,
    I: VmAdapterInterface<AB::Expr>,
    I::Reads: From<([AB::Expr; 2], AB::Expr)>,
    I::Writes: From<[AB::Expr; NUM_CELLS]>,
    I::ProcessedInstruction: From<NativeLoadStoreInstruction<AB::Expr>>,
{
    fn eval(
        &self,
        builder: &mut AB,
        local_core: &[AB::Var],
        _from_pc: AB::Var,
    ) -> AdapterAirContext<AB::Expr, I> {
        let cols: &NativeLoadStoreCoreCols<_, NUM_CELLS> = (*local_core).borrow();
        let flags = [
            cols.is_loadw,
            cols.is_storew,
            cols.is_loadw2,
            cols.is_storew2,
            cols.is_shintw,
        ];
        let is_valid = flags.iter().fold(AB::Expr::ZERO, |acc, &flag| {
            builder.assert_bool(flag);
            acc + flag.into()
        });
        builder.assert_bool(is_valid.clone());

        let expected_opcode = flags.iter().zip(NativeLoadStoreOpcode::iter()).fold(
            AB::Expr::ZERO,
            |acc, (flag, opcode)| {
                acc + (*flag).into() * AB::Expr::from_canonical_usize(opcode.as_usize())
            },
        ) + AB::Expr::from_canonical_usize(self.offset);

        AdapterAirContext {
            to_pc: None,
            reads: (cols.pointer_reads.map(Into::into), cols.data_read.into()).into(),
            writes: cols.data_write.map(Into::into).into(),
            instruction: NativeLoadStoreInstruction {
                is_valid,
                opcode: expected_opcode,
                is_loadw: cols.is_loadw.into(),
                is_storew: cols.is_storew.into(),
                is_loadw2: cols.is_loadw2.into(),
                is_storew2: cols.is_storew2.into(),
                is_shintw: cols.is_shintw.into(),
            }
            .into(),
        }
    }
}

#[derive(Debug)]
pub struct NativeLoadStoreCoreChip<F: Field, const NUM_CELLS: usize> {
    pub air: NativeLoadStoreCoreAir<NUM_CELLS>,
    pub streams: OnceLock<Arc<Mutex<Streams<F>>>>,
}

impl<F: Field, const NUM_CELLS: usize> NativeLoadStoreCoreChip<F, NUM_CELLS> {
    pub fn new(offset: usize) -> Self {
        Self {
            air: NativeLoadStoreCoreAir::<NUM_CELLS> { offset },
            streams: OnceLock::new(),
        }
    }
    pub fn set_streams(&mut self, streams: Arc<Mutex<Streams<F>>>) {
        self.streams.set(streams).unwrap();
    }
}

impl<F: PrimeField32, I: VmAdapterInterface<F>, const NUM_CELLS: usize> VmCoreChip<F, I>
    for NativeLoadStoreCoreChip<F, NUM_CELLS>
where
    I::Reads: Into<([F; 2], F)>,
    I::Writes: From<[F; NUM_CELLS]>,
{
    type Record = NativeLoadStoreCoreRecord<F, NUM_CELLS>;
    type Air = NativeLoadStoreCoreAir<NUM_CELLS>;

    fn execute_instruction(
        &self,
        instruction: &Instruction<F>,
        from_pc: u32,
        reads: I::Reads,
    ) -> Result<(AdapterRuntimeContext<F, I>, Self::Record)> {
        let Instruction { opcode, .. } = *instruction;
        let local_opcode =
            NativeLoadStoreOpcode::from_usize(opcode.local_opcode_idx(self.air.offset));
        let (pointer_reads, data_read) = reads.into();

        let data_write = if local_opcode == NativeLoadStoreOpcode::SHINTW {
            let mut streams = self.streams.get().unwrap().lock().unwrap();
            if streams.hint_stream.len() < NUM_CELLS {
                return Err(ExecutionError::HintOutOfBounds { pc: from_pc });
            }
            array::from_fn(|_| streams.hint_stream.pop_front().unwrap())
        } else {
            [data_read; NUM_CELLS]
        };

        let output = AdapterRuntimeContext::without_pc(data_write);
        let record = NativeLoadStoreCoreRecord {
            opcode: NativeLoadStoreOpcode::from_usize(opcode.local_opcode_idx(self.air.offset)),
            pointer_reads,
            data_read,
            data_write,
        };
        Ok((output, record))
    }

    fn get_opcode_name(&self, opcode: usize) -> String {
        format!(
            "{:?}",
            NativeLoadStoreOpcode::from_usize(opcode - self.air.offset)
        )
    }

    fn generate_trace_row(&self, row_slice: &mut [F], record: Self::Record) {
        let cols: &mut NativeLoadStoreCoreCols<_, NUM_CELLS> = row_slice.borrow_mut();
        cols.is_loadw = F::from_bool(record.opcode == NativeLoadStoreOpcode::LOADW);
        cols.is_storew = F::from_bool(record.opcode == NativeLoadStoreOpcode::STOREW);
        cols.is_loadw2 = F::from_bool(record.opcode == NativeLoadStoreOpcode::LOADW2);
        cols.is_storew2 = F::from_bool(record.opcode == NativeLoadStoreOpcode::STOREW2);
        cols.is_shintw = F::from_bool(record.opcode == NativeLoadStoreOpcode::SHINTW);

        cols.pointer_reads = record.pointer_reads.map(Into::into);
        cols.data_read = record.data_read;
        cols.data_write = record.data_write.map(Into::into);
    }

    fn air(&self) -> &Self::Air {
        &self.air
    }
}
