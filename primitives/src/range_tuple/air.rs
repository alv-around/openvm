use afs_stark_backend::interaction::InteractionBuilder;
use p3_air::{Air, BaseAir, PairBuilder};
use p3_field::Field;
use p3_matrix::{dense::RowMajorMatrix, Matrix};

use super::columns::{RangeTupleCols, RangeTuplePreprocessedCols, NUM_RANGE_TUPLE_COLS};
use crate::range_tuple::bus::RangeTupleCheckerBus;

#[derive(Clone, Default, Debug)]
pub struct RangeTupleCheckerAir {
    pub bus: RangeTupleCheckerBus,
}

impl RangeTupleCheckerAir {
    pub fn height(&self) -> u32 {
        self.bus.sizes.iter().product()
    }
}

impl<F: Field> BaseAir<F> for RangeTupleCheckerAir {
    fn width(&self) -> usize {
        NUM_RANGE_TUPLE_COLS
    }

    fn preprocessed_trace(&self) -> Option<RowMajorMatrix<F>> {
        let mut unrolled_matrix =
            Vec::with_capacity((self.height() as usize) * self.bus.sizes.len());
        let mut row = vec![0u32; self.bus.sizes.len()];
        for _ in 0..self.height() {
            unrolled_matrix.extend(row.clone());
            for i in (0..self.bus.sizes.len()).rev() {
                if row[i] < self.bus.sizes[i] - 1 {
                    row[i] += 1;
                    break;
                }
                row[i] = 0;
            }
        }
        Some(RowMajorMatrix::new(
            unrolled_matrix
                .iter()
                .map(|&v| F::from_canonical_u32(v))
                .collect(),
            self.bus.sizes.len(),
        ))
    }
}

impl<AB: InteractionBuilder + PairBuilder> Air<AB> for RangeTupleCheckerAir {
    fn eval(&self, builder: &mut AB) {
        let preprocessed = builder.preprocessed();
        let prep_local = preprocessed.row_slice(0);
        let prep_local = RangeTuplePreprocessedCols {
            tuple: (*prep_local).to_vec(),
        };
        let main = builder.main();
        let local = main.row_slice(0);
        let local = RangeTupleCols { mult: (*local)[0] };

        // Omit creating separate bridge.rs file for brevity
        self.bus.receive(prep_local.tuple).eval(builder, local.mult);
    }
}