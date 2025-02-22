use std::{
    array,
    borrow::{Borrow, BorrowMut},
    sync::{Arc, Mutex, OnceLock},
};

use openvm_circuit::arch::{
    AdapterAirContext, AdapterRuntimeContext, ExecutionError, MinimalInstruction, Result, Streams,
    VmAdapterInterface, VmCoreAir, VmCoreChip,
};
use openvm_circuit_primitives::bitwise_op_lookup::{
    BitwiseOperationLookupBus, BitwiseOperationLookupChip,
};
use openvm_circuit_primitives_derive::AlignedBorrow;
use openvm_instructions::{instruction::Instruction, UsizeOpcode};
use openvm_rv32im_transpiler::Rv32HintStoreOpcode;
use openvm_stark_backend::{
    interaction::InteractionBuilder,
    p3_air::BaseAir,
    p3_field::{Field, FieldAlgebra, PrimeField32},
    rap::BaseAirWithPublicValues,
};

use crate::adapters::{RV32_CELL_BITS, RV32_REGISTER_NUM_LIMBS};

/// HintStore Core Chip handles the range checking of the data to be written to memory
#[repr(C)]
#[derive(Debug, Clone, AlignedBorrow)]
pub struct Rv32HintStoreCoreCols<T> {
    pub is_valid: T,
    pub data: [T; RV32_REGISTER_NUM_LIMBS],
}

#[derive(Debug, Clone)]
pub struct Rv32HintStoreCoreRecord<F> {
    pub data: [F; RV32_REGISTER_NUM_LIMBS],
}

#[derive(Debug, Clone)]
pub struct Rv32HintStoreCoreAir {
    pub bus: BitwiseOperationLookupBus,
    pub offset: usize,
}

impl<F: Field> BaseAir<F> for Rv32HintStoreCoreAir {
    fn width(&self) -> usize {
        Rv32HintStoreCoreCols::<F>::width()
    }
}

impl<F: Field> BaseAirWithPublicValues<F> for Rv32HintStoreCoreAir {}

impl<AB, I> VmCoreAir<AB, I> for Rv32HintStoreCoreAir
where
    AB: InteractionBuilder,
    I: VmAdapterInterface<AB::Expr>,
    I::Reads: From<[[AB::Expr; RV32_REGISTER_NUM_LIMBS]; 0]>,
    I::Writes: From<[[AB::Expr; RV32_REGISTER_NUM_LIMBS]; 1]>,
    I::ProcessedInstruction: From<MinimalInstruction<AB::Expr>>,
{
    fn eval(
        &self,
        builder: &mut AB,
        local_core: &[AB::Var],
        _from_pc: AB::Var,
    ) -> AdapterAirContext<AB::Expr, I> {
        let cols: &Rv32HintStoreCoreCols<AB::Var> = (*local_core).borrow();

        builder.assert_bool(cols.is_valid);

        let expected_opcode =
            AB::Expr::from_canonical_usize(Rv32HintStoreOpcode::HINT_STOREW as usize)
                + AB::Expr::from_canonical_usize(self.offset);

        for i in 0..RV32_REGISTER_NUM_LIMBS / 2 {
            self.bus
                .send_range(cols.data[i * 2], cols.data[i * 2 + 1])
                .eval(builder, cols.is_valid);
        }

        AdapterAirContext {
            to_pc: None,
            reads: [].into(),
            writes: [cols.data.map(|x| x.into())].into(),
            instruction: MinimalInstruction {
                is_valid: cols.is_valid.into(),
                opcode: expected_opcode,
            }
            .into(),
        }
    }
}

#[derive(Debug)]
pub struct Rv32HintStoreCoreChip<F: Field> {
    pub air: Rv32HintStoreCoreAir,
    pub streams: OnceLock<Arc<Mutex<Streams<F>>>>,
    pub bitwise_lookup_chip: Arc<BitwiseOperationLookupChip<RV32_CELL_BITS>>,
}

impl<F: PrimeField32> Rv32HintStoreCoreChip<F> {
    pub fn new(
        bitwise_lookup_chip: Arc<BitwiseOperationLookupChip<RV32_CELL_BITS>>,
        offset: usize,
    ) -> Self {
        Self {
            air: Rv32HintStoreCoreAir {
                bus: bitwise_lookup_chip.bus(),
                offset,
            },
            streams: OnceLock::new(),
            bitwise_lookup_chip,
        }
    }
    pub fn set_streams(&mut self, streams: Arc<Mutex<Streams<F>>>) {
        self.streams.set(streams).unwrap();
    }
}

impl<F: PrimeField32, I: VmAdapterInterface<F>> VmCoreChip<F, I> for Rv32HintStoreCoreChip<F>
where
    I::Reads: Into<[[F; RV32_REGISTER_NUM_LIMBS]; 0]>,
    I::Writes: From<[[F; RV32_REGISTER_NUM_LIMBS]; 1]>,
{
    type Record = Rv32HintStoreCoreRecord<F>;
    type Air = Rv32HintStoreCoreAir;

    #[allow(clippy::type_complexity)]
    fn execute_instruction(
        &self,
        _instruction: &Instruction<F>,
        from_pc: u32,
        _reads: I::Reads,
    ) -> Result<(AdapterRuntimeContext<F, I>, Self::Record)> {
        let mut streams = self.streams.get().unwrap().lock().unwrap();
        if streams.hint_stream.len() < RV32_REGISTER_NUM_LIMBS {
            return Err(ExecutionError::HintOutOfBounds { pc: from_pc });
        }
        let data: [F; RV32_REGISTER_NUM_LIMBS] =
            array::from_fn(|_| streams.hint_stream.pop_front().unwrap());
        let write_data = data;

        let output = AdapterRuntimeContext::without_pc([write_data]);
        for i in 0..(RV32_REGISTER_NUM_LIMBS / 2) {
            self.bitwise_lookup_chip.request_range(
                write_data[2 * i].as_canonical_u32(),
                write_data[2 * i + 1].as_canonical_u32(),
            );
        }
        Ok((output, Rv32HintStoreCoreRecord { data: write_data }))
    }

    fn get_opcode_name(&self, opcode: usize) -> String {
        format!(
            "{:?}",
            Rv32HintStoreOpcode::from_usize(opcode - self.air.offset)
        )
    }

    fn generate_trace_row(&self, row_slice: &mut [F], record: Self::Record) {
        let core_cols: &mut Rv32HintStoreCoreCols<F> = row_slice.borrow_mut();
        core_cols.is_valid = F::ONE;
        core_cols.data = record.data;
    }

    fn air(&self) -> &Self::Air {
        &self.air
    }
}
