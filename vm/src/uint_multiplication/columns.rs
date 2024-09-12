use std::mem::size_of;

use afs_derive::AlignedBorrow;

use crate::{
    arch::columns::ExecutionState,
    memory::offline_checker::{MemoryReadAuxCols, MemoryWriteAuxCols},
};

#[repr(C)]
#[derive(AlignedBorrow)]
pub struct UintMultiplicationCols<T, const NUM_LIMBS: usize, const LIMB_BITS: usize> {
    pub io: UintMultiplicationIoCols<T, NUM_LIMBS, LIMB_BITS>,
    pub aux: UintMultiplicationAuxCols<T, NUM_LIMBS, LIMB_BITS>,
}

impl<T, const NUM_LIMBS: usize, const LIMB_BITS: usize>
    UintMultiplicationCols<T, NUM_LIMBS, LIMB_BITS>
{
    pub fn width() -> usize {
        UintMultiplicationAuxCols::<T, NUM_LIMBS, LIMB_BITS>::width()
            + UintMultiplicationIoCols::<T, NUM_LIMBS, LIMB_BITS>::width()
    }
}

#[repr(C)]
#[derive(AlignedBorrow)]
pub struct UintMultiplicationIoCols<T, const NUM_LIMBS: usize, const LIMB_BITS: usize> {
    pub from_state: ExecutionState<T>,
    pub x: MemoryData<T, NUM_LIMBS, LIMB_BITS>,
    pub y: MemoryData<T, NUM_LIMBS, LIMB_BITS>,
    pub z: MemoryData<T, NUM_LIMBS, LIMB_BITS>,
    pub ptr_as: T,
    pub address_as: T,
}

impl<T, const NUM_LIMBS: usize, const LIMB_BITS: usize>
    UintMultiplicationIoCols<T, NUM_LIMBS, LIMB_BITS>
{
    pub fn width() -> usize {
        size_of::<UintMultiplicationIoCols<u8, NUM_LIMBS, LIMB_BITS>>()
    }
}

#[repr(C)]
#[derive(AlignedBorrow)]
pub struct UintMultiplicationAuxCols<T, const NUM_LIMBS: usize, const LIMB_BITS: usize> {
    pub is_valid: T,
    pub carry: [T; NUM_LIMBS],
    pub read_ptr_aux_cols: [MemoryReadAuxCols<T, 1>; 3],
    pub read_x_aux_cols: MemoryReadAuxCols<T, NUM_LIMBS>,
    pub read_y_aux_cols: MemoryReadAuxCols<T, NUM_LIMBS>,
    pub write_z_aux_cols: MemoryWriteAuxCols<T, NUM_LIMBS>,
}

impl<T, const NUM_LIMBS: usize, const LIMB_BITS: usize>
    UintMultiplicationAuxCols<T, NUM_LIMBS, LIMB_BITS>
{
    pub fn width() -> usize {
        size_of::<UintMultiplicationAuxCols<u8, NUM_LIMBS, LIMB_BITS>>()
    }
}

#[repr(C)]
#[derive(AlignedBorrow)]
pub struct MemoryData<T, const NUM_LIMBS: usize, const LIMB_BITS: usize> {
    pub data: [T; NUM_LIMBS],
    pub address: T,
    pub ptr_to_address: T,
}

impl<T, const NUM_LIMBS: usize, const LIMB_BITS: usize> MemoryData<T, NUM_LIMBS, LIMB_BITS> {
    pub fn width() -> usize {
        size_of::<MemoryData<u8, NUM_LIMBS, LIMB_BITS>>()
    }
}
