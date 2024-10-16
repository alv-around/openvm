use crate::{
    arch::{VmAirWrapper, VmChipWrapper},
    kernels::adapters::convert_adapter::{ConvertAdapterAir, ConvertAdapterChip},
};

#[cfg(test)]
pub mod tests;

mod core;
pub use core::*;

pub type NewCastFAir = VmAirWrapper<ConvertAdapterAir<1, 4>, NewCastFCoreAir>;
pub type NewCastFChip<F> = VmChipWrapper<F, ConvertAdapterChip<F, 1, 4>, NewCastFCoreChip>;
