use p3_air::{
    AirBuilder, AirBuilderWithPublicValues, ExtensionBuilder, PairBuilder, PermutationAirBuilder,
};
use p3_matrix::dense::RowMajorMatrixView;
use p3_matrix::stack::VerticalPair;

use afs_compiler::ir::{Config, Ext, Felt, SymbolicExt};
use afs_stark_backend::air_builders::PartitionedAirBuilder;
use afs_stark_backend::rap::PermutationAirBuilderWithExposedValues;

type ViewPair<'a, T> = VerticalPair<RowMajorMatrixView<'a, T>, RowMajorMatrixView<'a, T>>;

type Var<C> = Ext<<C as Config>::F, <C as Config>::EF>;

pub struct RecursiveVerifierConstraintFolder<'a, C: Config> {
    pub preprocessed: ViewPair<'a, Var<C>>,
    pub partitioned_main: Vec<ViewPair<'a, Var<C>>>,
    pub after_challenge: Vec<ViewPair<'a, Var<C>>>,
    pub challenges: &'a [Vec<Var<C>>],
    pub is_first_row: Var<C>,
    pub is_last_row: Var<C>,
    pub is_transition: Var<C>,
    pub alpha: Var<C>,
    pub accumulator: SymbolicExt<C::F, C::EF>,
    pub public_values: &'a [Felt<C::F>],
    pub exposed_values_after_challenge: &'a [Vec<Var<C>>],
}

impl<'a, C: Config> AirBuilder for RecursiveVerifierConstraintFolder<'a, C> {
    type F = C::F;
    type Var = Ext<C::F, C::EF>;
    type Expr = SymbolicExt<C::F, C::EF>;
    type M = ViewPair<'a, Self::Var>;

    /// It is difficult to horizontally concatenate matrices when the main trace is partitioned, so we disable this method in that case.
    fn main(&self) -> Self::M {
        if self.partitioned_main.len() == 1 {
            self.partitioned_main[0]
        } else {
            panic!("Main trace is either empty or partitioned. This function should not be used.")
        }
    }

    fn is_first_row(&self) -> Self::Expr {
        self.is_first_row.into()
    }

    fn is_last_row(&self) -> Self::Expr {
        self.is_last_row.into()
    }

    fn is_transition_window(&self, size: usize) -> Self::Expr {
        if size == 2 {
            self.is_transition.into()
        } else {
            panic!("uni-stark only supports a window size of 2")
        }
    }

    fn assert_zero<I: Into<Self::Expr>>(&mut self, x: I) {
        let x = x.into();
        self.accumulator *= self.alpha;
        self.accumulator += x;
    }
}

impl<'a, C> PairBuilder for RecursiveVerifierConstraintFolder<'a, C>
where
    C: Config,
{
    fn preprocessed(&self) -> Self::M {
        self.preprocessed
    }
}

impl<'a, C> ExtensionBuilder for RecursiveVerifierConstraintFolder<'a, C>
where
    C: Config,
{
    type EF = C::EF;
    type ExprEF = Self::Expr;
    type VarEF = Self::Var;

    fn assert_zero_ext<I>(&mut self, x: I)
    where
        I: Into<Self::ExprEF>,
    {
        let x: Self::Expr = x.into();
        self.accumulator *= <Self::Expr>::from(self.alpha);
        self.accumulator += x;
    }
}

impl<'a, C> PermutationAirBuilder for RecursiveVerifierConstraintFolder<'a, C>
where
    C: Config,
{
    type MP = ViewPair<'a, Var<C>>;

    type RandomVar = Var<C>;

    fn permutation(&self) -> Self::MP {
        *self
            .after_challenge
            .first()
            .expect("Challenge phase not supported")
    }

    fn permutation_randomness(&self) -> &[Self::RandomVar] {
        self.challenges
            .first()
            .map(|c| c.as_slice())
            .expect("Challenge phase not supported")
    }
}

impl<'a, C: Config> AirBuilderWithPublicValues for RecursiveVerifierConstraintFolder<'a, C> {
    type PublicVar = Felt<C::F>;

    fn public_values(&self) -> &[Self::PublicVar] {
        self.public_values
    }
}

impl<'a, C> PermutationAirBuilderWithExposedValues for RecursiveVerifierConstraintFolder<'a, C>
where
    C: Config,
{
    fn permutation_exposed_values(&self) -> &[Self::Var] {
        self.exposed_values_after_challenge
            .first()
            .map(|c| c.as_slice())
            .expect("Challenge phase not supported")
    }
}

impl<'a, C> PartitionedAirBuilder for RecursiveVerifierConstraintFolder<'a, C>
where
    C: Config,
{
    fn partitioned_main(&self) -> &[Self::M] {
        &self.partitioned_main
    }
}