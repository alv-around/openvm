use openvm_native_circuit::execute_program;
use openvm_native_compiler::{
    asm::{AsmBuilder, AsmConfig},
    ir::{Array, RVar, SymbolicVar, Var},
};
use openvm_stark_backend::p3_field::{extension::BinomialExtensionField, FieldAlgebra};
use openvm_stark_sdk::p3_baby_bear::BabyBear;

type F = BabyBear;
type EF = BinomialExtensionField<BabyBear, 4>;

#[test]
fn test_compiler_for_loops() {
    let mut builder = AsmBuilder::<F, EF>::default();

    let n_val = BabyBear::from_canonical_u32(10);
    let m_val = BabyBear::from_canonical_u32(5);

    let zero: Var<_> = builder.eval(F::ZERO);
    let n: Var<_> = builder.eval(n_val);
    let m: Var<_> = builder.eval(m_val);

    let i_counter: Var<_> = builder.eval(F::ZERO);
    let total_counter: Var<_> = builder.eval(F::ZERO);
    builder.range(zero, n).for_each(|_, builder| {
        builder.assign(&i_counter, i_counter + F::ONE);

        let j_counter: Var<_> = builder.eval(F::ZERO);
        builder.range(zero, m).for_each(|_, builder| {
            builder.assign(&total_counter, total_counter + F::ONE);
            builder.assign(&j_counter, j_counter + F::ONE);
        });
        // Assert that the inner loop ran m times, in two different ways.
        builder.assert_var_eq(j_counter, m_val);
        builder.assert_var_eq(j_counter, m);
    });
    // Assert that the outer loop ran n times, in two different ways.
    builder.assert_var_eq(i_counter, n_val);
    builder.assert_var_eq(i_counter, n);
    // Assert that the total counter is equal to n * m, in two ways.
    builder.assert_var_eq(total_counter, n_val * m_val);
    builder.assert_var_eq(total_counter, n * m);

    builder.halt();

    let program = builder.compile_isa();
    execute_program(program, vec![]);
}

#[test]
fn test_compiler_nested_array_loop() {
    let mut builder = AsmBuilder::<F, EF>::default();
    type C = AsmConfig<F, EF>;

    let outer_len = 100;
    let inner_len = 10;

    let array: Array<C, Array<C, Var<_>>> = builder.dyn_array(outer_len);

    builder.range(0, array.len()).for_each(|i, builder| {
        let inner_array = builder.dyn_array::<Var<_>>(inner_len);
        builder.range(0, inner_array.len()).for_each(|j, builder| {
            builder.set(&inner_array, j, i + j); //(j * F::from_canonical_u16(300)));
        });
        builder.set(&array, i, inner_array);
    });

    // Test that the array is correctly initialized.
    builder.range(0, array.len()).for_each(|i, builder| {
        let inner_array = builder.get(&array, i);
        builder.range(0, inner_array.len()).for_each(|j, builder| {
            let val = builder.get(&inner_array, j);
            builder.assert_var_eq(val, i + j); //*(j * F::from_canonical_u16(300)));
        });
    });

    builder.halt();

    let program = builder.compile_isa();
    execute_program(program, vec![]);
    //execute_program(program, vec![]);
}

#[test]
fn test_compiler_break() {
    let mut builder = AsmBuilder::<F, EF>::default();
    type C = AsmConfig<F, EF>;

    let len = 100;
    let break_len = 10;

    let array: Array<C, Var<_>> = builder.dyn_array(len);

    builder
        .range(0, array.len())
        .may_break()
        .for_each(|i, builder| {
            builder.set(&array, i, i);

            builder
                .if_eq(i, RVar::from(break_len))
                .then_may_break(|builder| builder.break_loop())
        });

    // Test that the array is correctly initialized.

    builder
        .range(0, array.len())
        .may_break()
        .for_each(|i, builder| {
            let value = builder.get(&array, i);
            builder
                .if_eq(i, RVar::from(break_len + 1))
                .then_or_else_may_break(
                    |builder| {
                        builder.assert_var_eq(value, i);
                        Ok(())
                    },
                    |builder| {
                        builder.assert_var_eq(value, F::ZERO);
                        builder.break_loop()
                    },
                )
        });

    // Test the break instructions in a nested loop.

    let array: Array<C, Var<_>> = builder.dyn_array(len);
    builder
        .range(0, array.len())
        .may_break()
        .for_each(|i, builder| {
            let counter: Var<_> = builder.eval(F::ZERO);

            builder.range(0, i).may_break().for_each(|_, builder| {
                builder.assign(&counter, counter + F::ONE);
                builder
                    .if_eq(counter, RVar::from(break_len))
                    .then_may_break(|builder| builder.break_loop())
            });

            builder.set(&array, i, counter);
            Ok(())
        });

    // Test that the array is correctly initialized.

    let is_break: Var<_> = builder.eval(F::ONE);
    builder.range(0, array.len()).for_each(|i, builder| {
        let exp_value: Var<_> = builder.eval(
            i * is_break
                + (SymbolicVar::<F>::ONE - is_break)
                    * SymbolicVar::from(F::from_canonical_usize(break_len)),
        );
        let value = builder.get(&array, i);
        builder.assert_var_eq(value, exp_value);
        builder
            .if_eq(i, RVar::from(break_len))
            .then(|builder| builder.assign(&is_break, F::ZERO));
    });

    builder.halt();

    let program = builder.compile_isa();
    execute_program(program, vec![]);
}

#[test]
fn test_compiler_constant_break() {
    let mut builder = AsmBuilder::<F, EF>::default();
    builder.set_static_loops(true);
    type C = AsmConfig<F, EF>;

    let len = 100;
    let break_len = 10;

    let array: Array<C, Var<_>> = builder.uninit_fixed_array(len);
    builder
        .range(0, array.len())
        .may_break()
        .for_each(|i, builder| {
            builder.set(&array, i, i);

            builder
                .if_eq(i, RVar::from(break_len))
                .then_may_break(|builder| builder.break_loop())
        });
    builder.halt();

    let program = builder.compile_isa();
    execute_program(program, vec![]);
}

#[test]
#[should_panic]
fn test_compiler_constant_var_break() {
    let mut builder = AsmBuilder::<F, EF>::default();
    type C = AsmConfig<F, EF>;

    let len = 100;
    let break_len: Var<_> = builder.eval(RVar::from(10));

    let array: Array<C, Var<_>> = builder.uninit_fixed_array(len);
    builder
        .range(0, array.len())
        .may_break()
        .for_each(|i, builder| {
            builder.set(&array, i, i);

            builder
                .if_eq(i, RVar::from(break_len))
                .then_may_break(|builder| builder.break_loop())
        });
    builder.halt();

    let program = builder.compile_isa();
    execute_program(program, vec![]);
}

#[test]
fn test_compiler_step_by() {
    let mut builder = AsmBuilder::<F, EF>::default();

    let n_val = BabyBear::from_canonical_u32(20);

    let zero: Var<_> = builder.eval(F::ZERO);
    let n: Var<_> = builder.eval(n_val);

    let i_counter: Var<_> = builder.eval(F::ZERO);
    builder.range(zero, n).step_by(2).for_each(|_, builder| {
        builder.assign(&i_counter, i_counter + F::ONE);
    });
    // Assert that the outer loop ran n times, in two different ways.
    let n_exp = n_val / F::TWO;
    builder.assert_var_eq(i_counter, n_exp);

    builder.halt();

    let program = builder.compile_isa();
    execute_program(program, vec![]);
}

#[test]
fn test_compiler_bneinc() {
    let mut builder = AsmBuilder::<F, EF>::default();

    let n_val = BabyBear::from_canonical_u32(20);

    let zero: Var<_> = builder.eval(F::ZERO);
    let n: Var<_> = builder.eval(n_val);

    let i_counter: Var<_> = builder.eval(F::ZERO);
    builder.range(zero, n).step_by(1).for_each(|_, builder| {
        builder.assign(&i_counter, i_counter + F::ONE);
    });

    builder.halt();

    let program = builder.compile_isa();
    execute_program(program, vec![]);
}
