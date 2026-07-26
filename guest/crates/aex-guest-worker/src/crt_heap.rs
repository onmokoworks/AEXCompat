use std::collections::BTreeMap;
use std::fmt;

pub(crate) const CRT_HEAP_ALIGNMENT: u64 = 16;
pub(crate) const CRT_HEAP_PAGE_SIZE: u64 = 4096;
pub(crate) const MAX_CRT_ALLOCATION_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const MAX_CRT_HEAP_BYTES: u64 = 128 * 1024 * 1024;
pub(crate) const MAX_CRT_ALLOCATIONS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CrtAllocation {
    pub(crate) requested_size: u64,
    pub(crate) backing_size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CrtHeapError {
    SizeOverflow,
    AllocationTooLarge,
    AllocationCountExceeded,
    AggregateBudgetExceeded,
    AddressSpaceExhausted,
    InvalidPointer,
    DuplicatePointer,
    ForeignOrFreedPointer,
}

impl fmt::Display for CrtHeapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SizeOverflow => "CRT heap allocation size overflow",
            Self::AllocationTooLarge => "CRT heap per-allocation limit exceeded",
            Self::AllocationCountExceeded => "CRT heap allocation-count limit exceeded",
            Self::AggregateBudgetExceeded => "CRT heap aggregate budget exceeded",
            Self::AddressSpaceExhausted => "CRT heap guest address space exhausted",
            Self::InvalidPointer => "CRT heap allocator returned an invalid pointer",
            Self::DuplicatePointer => "CRT heap allocator returned a duplicate pointer",
            Self::ForeignOrFreedPointer => "CRT free rejected a foreign or already-freed pointer",
        })
    }
}

#[derive(Default)]
pub(crate) struct CrtHeap {
    allocations: BTreeMap<u64, CrtAllocation>,
    live_bytes: u64,
}

impl CrtHeap {
    pub(crate) fn checked_calloc_size(count: u64, element_size: u64) -> Result<u64, CrtHeapError> {
        count
            .checked_mul(element_size)
            .ok_or(CrtHeapError::SizeOverflow)
    }

    pub(crate) fn prepare_allocation(
        &self,
        requested_size: u64,
    ) -> Result<CrtAllocation, CrtHeapError> {
        let requested_size = requested_size.max(1);
        if requested_size > MAX_CRT_ALLOCATION_BYTES {
            return Err(CrtHeapError::AllocationTooLarge);
        }
        if self.allocations.len() >= MAX_CRT_ALLOCATIONS {
            return Err(CrtHeapError::AllocationCountExceeded);
        }
        if self.live_bytes > MAX_CRT_HEAP_BYTES - requested_size {
            return Err(CrtHeapError::AggregateBudgetExceeded);
        }
        let backing_size = align_up(requested_size, CRT_HEAP_PAGE_SIZE)?;
        Ok(CrtAllocation {
            requested_size,
            backing_size,
        })
    }

    pub(crate) fn first_fit(
        &self,
        range_start: u64,
        range_end: u64,
        allocation: CrtAllocation,
    ) -> Result<u64, CrtHeapError> {
        let mut candidate = align_up(range_start, CRT_HEAP_PAGE_SIZE)?;
        for (&pointer, existing) in self.allocations.range(range_start..range_end) {
            let candidate_end = candidate
                .checked_add(allocation.backing_size)
                .ok_or(CrtHeapError::AddressSpaceExhausted)?;
            if candidate_end <= pointer {
                return Ok(candidate);
            }
            candidate = align_up(
                pointer
                    .checked_add(existing.backing_size)
                    .ok_or(CrtHeapError::AddressSpaceExhausted)?,
                CRT_HEAP_PAGE_SIZE,
            )?;
        }
        candidate
            .checked_add(allocation.backing_size)
            .filter(|end| *end <= range_end)
            .map(|_| candidate)
            .ok_or(CrtHeapError::AddressSpaceExhausted)
    }

    pub(crate) fn insert(
        &mut self,
        pointer: u64,
        allocation: CrtAllocation,
    ) -> Result<(), CrtHeapError> {
        if pointer == 0 || pointer % CRT_HEAP_ALIGNMENT != 0 {
            return Err(CrtHeapError::InvalidPointer);
        }
        if self.allocations.contains_key(&pointer) {
            return Err(CrtHeapError::DuplicatePointer);
        }
        self.allocations.insert(pointer, allocation);
        self.live_bytes += allocation.requested_size;
        Ok(())
    }

    pub(crate) fn remove(&mut self, pointer: u64) -> Result<CrtAllocation, CrtHeapError> {
        let allocation = self
            .allocations
            .remove(&pointer)
            .ok_or(CrtHeapError::ForeignOrFreedPointer)?;
        self.live_bytes -= allocation.requested_size;
        Ok(allocation)
    }

    pub(crate) fn allocations(&self) -> impl Iterator<Item = (u64, CrtAllocation)> + '_ {
        self.allocations
            .iter()
            .map(|(&pointer, &allocation)| (pointer, allocation))
    }

    #[cfg(test)]
    pub(crate) fn live_bytes(&self) -> u64 {
        self.live_bytes
    }
}

fn align_up(value: u64, alignment: u64) -> Result<u64, CrtHeapError> {
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
        .ok_or(CrtHeapError::SizeOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_size_is_unique_minimum_allocation_and_page_backed() {
        let heap = CrtHeap::default();
        let allocation = heap.prepare_allocation(0).unwrap();
        assert_eq!(allocation.requested_size, 1);
        assert_eq!(allocation.backing_size, CRT_HEAP_PAGE_SIZE);
    }

    #[test]
    fn calloc_overflow_is_rejected() {
        assert_eq!(
            CrtHeap::checked_calloc_size(u64::MAX, 2),
            Err(CrtHeapError::SizeOverflow)
        );
    }

    #[test]
    fn limits_are_enforced_before_backend_mapping() {
        let mut heap = CrtHeap::default();
        assert_eq!(
            heap.prepare_allocation(MAX_CRT_ALLOCATION_BYTES + 1),
            Err(CrtHeapError::AllocationTooLarge)
        );
        for index in 0..2 {
            let allocation = heap.prepare_allocation(MAX_CRT_ALLOCATION_BYTES).unwrap();
            heap.insert(0x1000 + index * 0x1000, allocation).unwrap();
        }
        assert_eq!(heap.live_bytes(), MAX_CRT_HEAP_BYTES);
        assert_eq!(
            heap.prepare_allocation(1),
            Err(CrtHeapError::AggregateBudgetExceeded)
        );
    }

    #[test]
    fn allocation_count_limit_is_enforced() {
        let mut heap = CrtHeap::default();
        for index in 0..MAX_CRT_ALLOCATIONS {
            let allocation = heap.prepare_allocation(1).unwrap();
            heap.insert(0x1000 + index as u64 * CRT_HEAP_PAGE_SIZE, allocation)
                .unwrap();
        }
        assert_eq!(
            heap.prepare_allocation(1),
            Err(CrtHeapError::AllocationCountExceeded)
        );
    }

    #[test]
    fn first_fit_reuses_freed_page_and_preserves_alignment() {
        let mut heap = CrtHeap::default();
        let allocation = heap.prepare_allocation(17).unwrap();
        let first = heap.first_fit(0x10_0000, 0x20_0000, allocation).unwrap();
        assert_eq!(first % CRT_HEAP_ALIGNMENT, 0);
        heap.insert(first, allocation).unwrap();
        let second = heap.first_fit(0x10_0000, 0x20_0000, allocation).unwrap();
        heap.insert(second, allocation).unwrap();
        assert_eq!(second, first + CRT_HEAP_PAGE_SIZE);
        heap.remove(first).unwrap();
        assert_eq!(
            heap.first_fit(0x10_0000, 0x20_0000, allocation).unwrap(),
            first
        );
    }

    #[test]
    fn foreign_and_double_free_are_rejected() {
        let mut heap = CrtHeap::default();
        let allocation = heap.prepare_allocation(8).unwrap();
        heap.insert(0x1000, allocation).unwrap();
        assert_eq!(
            heap.remove(0x2000),
            Err(CrtHeapError::ForeignOrFreedPointer)
        );
        assert_eq!(heap.remove(0x1000), Ok(allocation));
        assert_eq!(
            heap.remove(0x1000),
            Err(CrtHeapError::ForeignOrFreedPointer)
        );
    }
}
