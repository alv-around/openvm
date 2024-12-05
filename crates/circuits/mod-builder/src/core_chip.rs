use std::sync::Arc;

use ax_circuit_primitives::{var_range::VariableRangeCheckerChip, SubAir, TraceSubRowGenerator};
use ax_stark_backend::{interaction::InteractionBuilder, rap::BaseAirWithPublicValues};
use axvm_circuit::arch::{
    AdapterAirContext, AdapterRuntimeContext, DynAdapterInterface, DynArray, MinimalInstruction,
    Result, VmAdapterInterface, VmCoreAir, VmCoreChip,
};
use axvm_instructions::instruction::Instruction;
use itertools::Itertools;
use num_bigint_dig::BigUint;
use p3_air::BaseAir;
use p3_field::{AbstractField, Field, PrimeField32};

use crate::{
    utils::{biguint_to_limbs_vec, limbs_to_biguint},
    FieldExpr, FieldExprCols,
};

#[derive(Clone)]
pub struct FieldExpressionCoreAir {
    pub expr: FieldExpr,

    /// The global opcode offset.
    pub offset: usize,

    /// All the opcode indices (including setup) supported by this Air.
    /// The last one must be the setup opcode if it's a chip needs setup.
    pub local_opcode_idx: Vec<usize>,
    /// Opcode flag idx (indices from builder.new_flag()) for all except setup opcode. Empty if single op chip.
    pub opcode_flag_idx: Vec<usize>,
    // Example 1: 1-op chip EcAdd that nees setup
    //   local_opcode_idx = [0, 2], where 0 is EcAdd, 2 is setup
    //   opcode_flag_idx = [], not needed for single op chip.
    // Example 2: 1-op chip EvaluateLine that doesn't need setup
    //   local_opcode_idx = [2], the id within PairingOpcodeEnum
    //   opcode_flag_idx = [], not needed
    // Example 3: 2-op chip MulDiv that needs setup
    //   local_opcode_idx = [2, 3, 4], where 2 is Mul, 3 is Div, 4 is setup
    //   opcode_flag_idx = [0, 1], where 0 is mul_flag, 1 is div_flag, in the builder
    // We don't support 2-op chip that doesn't need setup right now.
}

impl FieldExpressionCoreAir {
    pub fn new(
        expr: FieldExpr,
        offset: usize,
        local_opcode_idx: Vec<usize>,
        opcode_flag_idx: Vec<usize>,
    ) -> Self {
        let opcode_flag_idx = if opcode_flag_idx.is_empty() && expr.needs_setup() {
            // single op chip that needs setup, so there is only one default flag, must be 0.
            vec![0]
        } else {
            // multi ops chip or no-setup chip, use as is.
            opcode_flag_idx
        };
        assert_eq!(opcode_flag_idx.len(), local_opcode_idx.len() - 1);
        Self {
            expr,
            offset,
            local_opcode_idx,
            opcode_flag_idx,
        }
    }

    pub fn num_inputs(&self) -> usize {
        self.expr.builder.num_input
    }

    pub fn num_vars(&self) -> usize {
        self.expr.builder.num_variables
    }

    pub fn num_flags(&self) -> usize {
        self.expr.builder.num_flags
    }

    pub fn output_indices(&self) -> &[usize] {
        &self.expr.builder.output_indices
    }
}

impl<F: Field> BaseAir<F> for FieldExpressionCoreAir {
    fn width(&self) -> usize {
        BaseAir::<F>::width(&self.expr)
    }
}

impl<F: Field> BaseAirWithPublicValues<F> for FieldExpressionCoreAir {}

impl<AB: InteractionBuilder, I> VmCoreAir<AB, I> for FieldExpressionCoreAir
where
    I: VmAdapterInterface<AB::Expr>,
    AdapterAirContext<AB::Expr, I>:
        From<AdapterAirContext<AB::Expr, DynAdapterInterface<AB::Expr>>>,
{
    fn eval(
        &self,
        builder: &mut AB,
        local: &[AB::Var],
        _from_pc: AB::Var,
    ) -> AdapterAirContext<AB::Expr, I> {
        assert_eq!(local.len(), BaseAir::<AB::F>::width(&self.expr));
        self.expr.eval(builder, local);
        let FieldExprCols {
            is_valid,
            inputs,
            vars,
            flags,
            ..
        } = self.expr.load_vars(local);
        assert_eq!(inputs.len(), self.num_inputs());
        assert_eq!(vars.len(), self.num_vars());
        assert_eq!(flags.len(), self.num_flags());
        let reads: Vec<AB::Expr> = inputs.concat().iter().map(|x| (*x).into()).collect();
        let writes: Vec<AB::Expr> = self
            .output_indices()
            .iter()
            .map(|&i| vars[i].clone())
            .collect::<Vec<_>>()
            .concat()
            .iter()
            .map(|x| (*x).into())
            .collect();

        let opcode_flags_except_last = self.opcode_flag_idx.iter().map(|&i| flags[i]).collect_vec();
        let last_opcode_flag = is_valid
            - opcode_flags_except_last
                .iter()
                .map(|&v| v.into())
                .sum::<AB::Expr>();
        builder.assert_bool(last_opcode_flag.clone());
        let opcode_flags = opcode_flags_except_last
            .into_iter()
            .map(Into::into)
            .chain(Some(last_opcode_flag));
        let expected_opcode = opcode_flags
            .zip(self.local_opcode_idx.iter().map(|&i| i + self.offset))
            .map(|(flag, global_idx)| flag * AB::Expr::from_canonical_usize(global_idx))
            .sum();

        let instruction = MinimalInstruction {
            is_valid: is_valid.into(),
            opcode: expected_opcode,
        };

        let ctx: AdapterAirContext<_, DynAdapterInterface<_>> = AdapterAirContext {
            to_pc: None,
            reads: reads.into(),
            writes: writes.into(),
            instruction: instruction.into(),
        };
        ctx.into()
    }
}

pub struct FieldExpressionRecord {
    pub inputs: Vec<BigUint>,
    pub flags: Vec<bool>,
}

pub struct FieldExpressionCoreChip {
    pub air: FieldExpressionCoreAir,
    pub range_checker: Arc<VariableRangeCheckerChip>,

    pub name: String,
}

impl FieldExpressionCoreChip {
    pub fn new(
        expr: FieldExpr,
        offset: usize,
        local_opcode_idx: Vec<usize>,
        opcode_flag_idx: Vec<usize>,
        range_checker: Arc<VariableRangeCheckerChip>,
        name: &str,
    ) -> Self {
        let air = FieldExpressionCoreAir::new(expr, offset, local_opcode_idx, opcode_flag_idx);
        Self {
            air,
            range_checker,
            name: name.to_string(),
        }
    }

    pub fn expr(&self) -> &FieldExpr {
        &self.air.expr
    }
}

impl<F: PrimeField32, I> VmCoreChip<F, I> for FieldExpressionCoreChip
where
    I: VmAdapterInterface<F>,
    I::Reads: Into<DynArray<F>>,
    AdapterRuntimeContext<F, I>: From<AdapterRuntimeContext<F, DynAdapterInterface<F>>>,
{
    type Record = FieldExpressionRecord;
    type Air = FieldExpressionCoreAir;

    fn execute_instruction(
        &self,
        instruction: &Instruction<F>,
        _from_pc: u32,
        reads: I::Reads,
    ) -> Result<(AdapterRuntimeContext<F, I>, Self::Record)> {
        let field_element_limbs = self.air.expr.canonical_num_limbs();
        let limb_bits = self.air.expr.canonical_limb_bits();
        let data: DynArray<_> = reads.into();
        let data = data.0;
        assert_eq!(data.len(), self.air.num_inputs() * field_element_limbs);
        let data_u32: Vec<u32> = data.iter().map(|x| x.as_canonical_u32()).collect();

        let mut inputs = vec![];
        for i in 0..self.air.num_inputs() {
            let start = i * field_element_limbs;
            let end = start + field_element_limbs;
            let limb_slice = &data_u32[start..end];
            let input = limbs_to_biguint(limb_slice, limb_bits);
            inputs.push(input);
        }

        let Instruction { opcode, .. } = instruction.clone();
        let local_opcode_index = opcode - self.air.offset;
        let mut flags = vec![];

        // If the chip doesn't need setup, (right now) it must be single op chip and thus no flag is needed.
        // Otherwise, there is a flag for each opcode and will be derived by is_valid - sum(flags).
        if self.expr().needs_setup() {
            flags = vec![false; self.air.num_flags()];
            self.air
                .opcode_flag_idx
                .iter()
                .enumerate()
                .for_each(|(i, &flag_idx)| {
                    flags[flag_idx] = local_opcode_index == self.air.local_opcode_idx[i]
                });
        }

        let vars = self.air.expr.execute(inputs.clone(), flags.clone());
        assert_eq!(vars.len(), self.air.num_vars());

        let outputs: Vec<BigUint> = self
            .air
            .output_indices()
            .iter()
            .map(|&i| vars[i].clone())
            .collect();
        let writes: Vec<F> = outputs
            .iter()
            .map(|x| biguint_to_limbs_vec(x.clone(), limb_bits, field_element_limbs))
            .concat()
            .into_iter()
            .map(|x| F::from_canonical_u32(x))
            .collect();

        let ctx = AdapterRuntimeContext::<_, DynAdapterInterface<_>>::without_pc(writes);
        Ok((ctx.into(), FieldExpressionRecord { inputs, flags }))
    }

    fn get_opcode_name(&self, _opcode: usize) -> String {
        self.name.clone()
    }

    fn generate_trace_row(&self, row_slice: &mut [F], record: Self::Record) {
        self.air.expr.generate_subrow(
            (&self.range_checker, record.inputs, record.flags),
            row_slice,
        );
    }

    fn air(&self) -> &Self::Air {
        &self.air
    }
}