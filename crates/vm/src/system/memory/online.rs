use std::{array, fmt::Debug};

use openvm_stark_backend::p3_field::PrimeField32;
use rustc_hash::FxHashMap;

use crate::system::memory::{offline::INITIAL_TIMESTAMP, MemoryImage, RecordId};

#[derive(Debug, Clone)]
pub enum MemoryLogEntry<T> {
    Read {
        address_space: u32,
        pointer: u32,
        len: usize,
    },
    Write {
        address_space: u32,
        pointer: u32,
        data: Vec<T>,
    },
    IncrementTimestampBy(u32),
}

/// (address_space, pointer)
pub(crate) type Address = (u32, u32);

/// A simple data structure to read to/write from memory.
///
/// Stores a log of memory accesses to reconstruct aspects of memory state for trace generation.
#[derive(Debug, Clone)]
pub struct Memory<F> {
    pub(super) data: FxHashMap<Address, F>,
    pub(super) log: Vec<MemoryLogEntry<F>>,
    timestamp: u32,
}

impl<F: PrimeField32> Default for Memory<F> {
    fn default() -> Self {
        Self::new()
    }
}

impl<F: PrimeField32> Memory<F> {
    pub fn new() -> Self {
        Self {
            data: MemoryImage::default(),
            timestamp: INITIAL_TIMESTAMP + 1,
            log: vec![],
        }
    }

    /// Instantiates a new `Memory` data structure from an image.
    pub fn from_image(image: MemoryImage<F>) -> Self {
        Self {
            data: image,
            timestamp: INITIAL_TIMESTAMP + 1,
            log: vec![],
        }
    }

    fn last_record_id(&self) -> RecordId {
        RecordId(self.log.len() - 1)
    }

    /// Writes an array of values to the memory at the specified address space and start index.
    ///
    /// Returns the `RecordId` for the memory record and the previous data.
    pub fn write<const N: usize>(
        &mut self,
        address_space: u32,
        pointer: u32,
        values: [F; N],
    ) -> (RecordId, [F; N]) {
        assert!(N.is_power_of_two());

        let prev_data = array::from_fn(|i| {
            self.data
                .insert((address_space, pointer + i as u32), values[i])
                .unwrap_or(F::ZERO)
        });

        self.log.push(MemoryLogEntry::Write {
            address_space,
            pointer,
            data: values.to_vec(),
        });
        self.timestamp += 1;

        (self.last_record_id(), prev_data)
    }

    /// Reads an array of values from the memory at the specified address space and start index.
    pub fn read<const N: usize>(&mut self, address_space: u32, pointer: u32) -> (RecordId, [F; N]) {
        assert!(N.is_power_of_two());

        self.log.push(MemoryLogEntry::Read {
            address_space,
            pointer,
            len: N,
        });

        let values = if address_space == 0 {
            assert_eq!(N, 1, "cannot batch read from address space 0");
            [F::from_canonical_u32(pointer); N]
        } else {
            self.range_array::<N>(address_space, pointer)
        };
        self.timestamp += 1;
        (self.last_record_id(), values)
    }

    pub fn increment_timestamp_by(&mut self, amount: u32) {
        self.timestamp += amount;
        self.log.push(MemoryLogEntry::IncrementTimestampBy(amount))
    }

    pub fn timestamp(&self) -> u32 {
        self.timestamp
    }

    pub fn get(&self, address_space: u32, pointer: u32) -> F {
        *self.data.get(&(address_space, pointer)).unwrap_or(&F::ZERO)
    }

    fn range_array<const N: usize>(&self, address_space: u32, pointer: u32) -> [F; N] {
        array::from_fn(|i| self.get(address_space, pointer + i as u32))
    }
}

#[cfg(test)]
mod tests {
    use openvm_stark_backend::p3_field::FieldAlgebra;
    use openvm_stark_sdk::p3_baby_bear::BabyBear;

    use super::Memory;

    macro_rules! bba {
        [$($x:expr),*] => {
            [$(BabyBear::from_canonical_u32($x)),*]
        }
    }

    #[test]
    fn test_write_read() {
        let mut memory = Memory::new();
        let address_space = 1;

        memory.write(address_space, 0, bba![1, 2, 3, 4]);

        let (_, data) = memory.read::<2>(address_space, 0);
        assert_eq!(data, bba![1, 2]);

        memory.write(address_space, 2, bba![100]);

        let (_, data) = memory.read::<4>(address_space, 0);
        assert_eq!(data, bba![1, 2, 100, 4]);
    }
}
