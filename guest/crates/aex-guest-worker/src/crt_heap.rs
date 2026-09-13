use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;

pub(crate) const CRT_HEAP_ALIGNMENT: u64 = 16;
pub(crate) const CRT_HEAP_PAGE_SIZE: u64 = 4096;
pub(crate) const MAX_CRT_ALLOCATION_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const MAX_CRT_HEAP_BYTES: u64 = 128 * 1024 * 1024;
// Sapphire setup holds more than 4096 small C++ objects concurrently. Keep
// a finite metadata/page-overhead bound while retaining the byte-size limits.
pub(crate) const MAX_CRT_ALLOCATIONS: usize = 32_768;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CrtAllocation {
    pub(crate) requested_size: u64,
    pub(crate) backing_size: u64,
    kind: CrtAllocationKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CrtAllocationKind {
    Regular,
    Aligned,
    ProcessHeap,
    EnvironmentStrings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CrtHeapError {
    SizeOverflow,
    AllocationTooLarge,
    AllocationCountExceeded,
    AggregateBudgetExceeded,
    AddressSpaceExhausted,
    InvalidAlignment,
    InvalidPointer,
    DuplicatePointer,
    ForeignOrFreedPointer,
    AllocatorMismatch,
}

impl fmt::Display for CrtHeapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SizeOverflow => "CRT heap allocation size overflow",
            Self::AllocationTooLarge => "CRT heap per-allocation limit exceeded",
            Self::AllocationCountExceeded => "CRT heap allocation-count limit exceeded",
            Self::AggregateBudgetExceeded => "CRT heap aggregate budget exceeded",
            Self::AddressSpaceExhausted => "CRT heap guest address space exhausted",
            Self::InvalidAlignment => {
                "CRT aligned allocation requires a nonzero power-of-two alignment"
            }
            Self::InvalidPointer => "CRT heap allocator returned an invalid pointer",
            Self::DuplicatePointer => "CRT heap allocator returned a duplicate pointer",
            Self::ForeignOrFreedPointer => "CRT free rejected a foreign or already-freed pointer",
            Self::AllocatorMismatch => "CRT free API does not own this allocation",
        })
    }
}

#[derive(Default)]
pub(crate) struct CrtHeap {
    allocations: BTreeMap<u64, CrtAllocation>,
    allocation_hints: RefCell<BTreeMap<(u64, u64), u64>>,
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
        self.prepare_allocation_kind(requested_size, CrtAllocationKind::Regular)
    }

    pub(crate) fn prepare_aligned_allocation(
        &self,
        requested_size: u64,
    ) -> Result<CrtAllocation, CrtHeapError> {
        self.prepare_allocation_kind(requested_size, CrtAllocationKind::Aligned)
    }

    pub(crate) fn prepare_process_heap_allocation(
        &self,
        requested_size: u64,
    ) -> Result<CrtAllocation, CrtHeapError> {
        self.prepare_allocation_kind(requested_size, CrtAllocationKind::ProcessHeap)
    }

    pub(crate) fn prepare_environment_strings_allocation(
        &self,
        requested_size: u64,
    ) -> Result<CrtAllocation, CrtHeapError> {
        self.prepare_allocation_kind(requested_size, CrtAllocationKind::EnvironmentStrings)
    }

    fn prepare_allocation_kind(
        &self,
        requested_size: u64,
        kind: CrtAllocationKind,
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
            kind,
        })
    }

    pub(crate) fn first_fit(
        &self,
        range_start: u64,
        range_end: u64,
        allocation: CrtAllocation,
    ) -> Result<u64, CrtHeapError> {
        self.first_fit_aligned(range_start, range_end, allocation, CRT_HEAP_PAGE_SIZE)
    }

    pub(crate) fn first_fit_aligned(
        &self,
        range_start: u64,
        range_end: u64,
        allocation: CrtAllocation,
        alignment: u64,
    ) -> Result<u64, CrtHeapError> {
        if alignment == 0 || !alignment.is_power_of_two() {
            return Err(CrtHeapError::InvalidAlignment);
        }
        let mapping_alignment = alignment.max(CRT_HEAP_PAGE_SIZE);
        let hinted = self
            .allocation_hints
            .borrow()
            .get(&(range_start, range_end))
            .copied()
            .filter(|hint| (range_start..range_end).contains(hint))
            .unwrap_or(range_start);
        let mut candidate = align_up(hinted, mapping_alignment)?;
        if let Some((&pointer, existing)) = self.allocations.range(..=candidate).next_back() {
            let existing_end = pointer
                .checked_add(existing.backing_size)
                .ok_or(CrtHeapError::AddressSpaceExhausted)?;
            if existing_end > candidate {
                candidate = align_up(existing_end, mapping_alignment)?;
            }
        }
        for (&pointer, existing) in self.allocations.range(candidate..range_end) {
            let candidate_end = candidate
                .checked_add(allocation.backing_size)
                .ok_or(CrtHeapError::AddressSpaceExhausted)?;
            if candidate_end <= pointer {
                self.allocation_hints
                    .borrow_mut()
                    .insert((range_start, range_end), candidate_end);
                return Ok(candidate);
            }
            candidate = align_up(
                pointer
                    .checked_add(existing.backing_size)
                    .ok_or(CrtHeapError::AddressSpaceExhausted)?,
                mapping_alignment,
            )?;
        }
        let selected = candidate
            .checked_add(allocation.backing_size)
            .filter(|end| *end <= range_end)
            .map(|_| candidate)
            .ok_or(CrtHeapError::AddressSpaceExhausted)?;
        self.allocation_hints
            .borrow_mut()
            .insert((range_start, range_end), selected + allocation.backing_size);
        Ok(selected)
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
        self.remove_kind(pointer, CrtAllocationKind::Regular)
    }

    pub(crate) fn process_heap_allocation(
        &self,
        pointer: u64,
    ) -> Result<CrtAllocation, CrtHeapError> {
        let allocation = *self
            .allocations
            .get(&pointer)
            .ok_or(CrtHeapError::ForeignOrFreedPointer)?;
        if allocation.kind != CrtAllocationKind::ProcessHeap {
            return Err(CrtHeapError::AllocatorMismatch);
        }
        Ok(allocation)
    }

    pub(crate) fn prepare_process_heap_reallocation(
        &self,
        pointer: u64,
        requested_size: u64,
    ) -> Result<CrtAllocation, CrtHeapError> {
        let old = self.process_heap_allocation(pointer)?;
        let requested_size = requested_size.max(1);
        if requested_size > MAX_CRT_ALLOCATION_BYTES {
            return Err(CrtHeapError::AllocationTooLarge);
        }
        let retained_bytes = self.live_bytes - old.requested_size;
        if retained_bytes > MAX_CRT_HEAP_BYTES - requested_size {
            return Err(CrtHeapError::AggregateBudgetExceeded);
        }
        let backing_size = align_up(requested_size, CRT_HEAP_PAGE_SIZE)?;
        Ok(CrtAllocation {
            requested_size,
            backing_size,
            kind: CrtAllocationKind::ProcessHeap,
        })
    }

    pub(crate) fn prepare_process_heap_in_place_reallocation(
        &self,
        pointer: u64,
        requested_size: u64,
    ) -> Result<Option<CrtAllocation>, CrtHeapError> {
        let old = self.process_heap_allocation(pointer)?;
        let mut replacement = self.prepare_process_heap_reallocation(pointer, requested_size)?;
        if replacement.backing_size > old.backing_size {
            return Ok(None);
        }
        replacement.backing_size = old.backing_size;
        Ok(Some(replacement))
    }

    pub(crate) fn commit_process_heap_reallocation(
        &mut self,
        old_pointer: u64,
        new_pointer: u64,
        allocation: CrtAllocation,
    ) -> Result<CrtAllocation, CrtHeapError> {
        let old = self.process_heap_allocation(old_pointer)?;
        if allocation.kind != CrtAllocationKind::ProcessHeap {
            return Err(CrtHeapError::AllocatorMismatch);
        }
        if new_pointer == 0 || new_pointer % CRT_HEAP_ALIGNMENT != 0 {
            return Err(CrtHeapError::InvalidPointer);
        }
        if new_pointer != old_pointer && self.allocations.contains_key(&new_pointer) {
            return Err(CrtHeapError::DuplicatePointer);
        }
        self.allocations.remove(&old_pointer);
        self.allocations.insert(new_pointer, allocation);
        self.live_bytes = self.live_bytes - old.requested_size + allocation.requested_size;
        if new_pointer != old_pointer {
            self.rewind_allocation_hints(old_pointer);
        }
        Ok(old)
    }

    pub(crate) fn remove_aligned(&mut self, pointer: u64) -> Result<CrtAllocation, CrtHeapError> {
        self.remove_kind(pointer, CrtAllocationKind::Aligned)
    }

    pub(crate) fn remove_process_heap(
        &mut self,
        pointer: u64,
    ) -> Result<CrtAllocation, CrtHeapError> {
        self.remove_kind(pointer, CrtAllocationKind::ProcessHeap)
    }

    pub(crate) fn remove_environment_strings(
        &mut self,
        pointer: u64,
    ) -> Result<CrtAllocation, CrtHeapError> {
        self.remove_kind(pointer, CrtAllocationKind::EnvironmentStrings)
    }

    fn remove_kind(
        &mut self,
        pointer: u64,
        expected: CrtAllocationKind,
    ) -> Result<CrtAllocation, CrtHeapError> {
        let allocation = *self
            .allocations
            .get(&pointer)
            .ok_or(CrtHeapError::ForeignOrFreedPointer)?;
        if allocation.kind != expected {
            return Err(CrtHeapError::AllocatorMismatch);
        }
        self.allocations.remove(&pointer);
        self.live_bytes -= allocation.requested_size;
        self.rewind_allocation_hints(pointer);
        Ok(allocation)
    }

    fn rewind_allocation_hints(&self, pointer: u64) {
        for (&(start, end), hint) in self.allocation_hints.borrow_mut().iter_mut() {
            if (start..end).contains(&pointer) && pointer < *hint {
                *hint = pointer;
            }
        }
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
        // Releasing one live object restores exactly one allocation slot.
        heap.remove(0x1000).unwrap();
        let replacement = heap.prepare_allocation(32).unwrap();
        heap.insert(0x1000, replacement).unwrap();
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
    fn aligned_first_fit_honors_large_alignment_and_free_kind() {
        let mut heap = CrtHeap::default();
        let aligned = heap.prepare_aligned_allocation(17).unwrap();
        let pointer = heap
            .first_fit_aligned(0x10_1000, 0x40_0000, aligned, 0x20_000)
            .unwrap();
        assert_eq!(pointer % 0x20_000, 0);
        heap.insert(pointer, aligned).unwrap();
        assert_eq!(heap.remove(pointer), Err(CrtHeapError::AllocatorMismatch));
        assert_eq!(heap.remove_aligned(pointer), Ok(aligned));

        let regular = heap.prepare_allocation(17).unwrap();
        heap.insert(0x10_0000, regular).unwrap();
        assert_eq!(
            heap.remove_aligned(0x10_0000),
            Err(CrtHeapError::AllocatorMismatch)
        );
        assert_eq!(heap.remove(0x10_0000), Ok(regular));
    }

    #[test]
    fn aligned_first_fit_rejects_invalid_alignment() {
        let heap = CrtHeap::default();
        let allocation = heap.prepare_aligned_allocation(1).unwrap();
        for alignment in [0, 3] {
            assert_eq!(
                heap.first_fit_aligned(0x10_0000, 0x20_0000, allocation, alignment),
                Err(CrtHeapError::InvalidAlignment)
            );
        }
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

    #[test]
    fn reallocation_replaces_ownership_and_accounts_only_the_size_delta() {
        let mut heap = CrtHeap::default();
        let old = heap
            .prepare_process_heap_allocation(MAX_CRT_ALLOCATION_BYTES)
            .unwrap();
        heap.insert(0x1000, old).unwrap();
        let peer = heap.prepare_allocation(MAX_CRT_ALLOCATION_BYTES).unwrap();
        heap.insert(0x2000, peer).unwrap();

        let replacement = heap.prepare_process_heap_reallocation(0x1000, 16).unwrap();
        assert_eq!(
            heap.commit_process_heap_reallocation(0x1000, 0x3000, replacement),
            Ok(old)
        );
        assert_eq!(
            heap.process_heap_allocation(0x1000),
            Err(CrtHeapError::ForeignOrFreedPointer)
        );
        assert_eq!(heap.process_heap_allocation(0x3000), Ok(replacement));
        assert_eq!(heap.live_bytes(), MAX_CRT_ALLOCATION_BYTES + 16);
    }

    #[test]
    fn in_place_reallocation_preserves_the_existing_mapping_extent() {
        let mut heap = CrtHeap::default();
        let old = heap.prepare_process_heap_allocation(4096).unwrap();
        heap.insert(0x1000, old).unwrap();

        let smaller = heap
            .prepare_process_heap_in_place_reallocation(0x1000, 8)
            .unwrap()
            .unwrap();
        assert_eq!(smaller.requested_size, 8);
        assert_eq!(smaller.backing_size, old.backing_size);
        assert_eq!(
            heap.prepare_process_heap_in_place_reallocation(0x1000, old.backing_size + 1),
            Ok(None)
        );
    }
}
