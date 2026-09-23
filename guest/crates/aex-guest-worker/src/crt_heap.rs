use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;

pub(crate) const CRT_HEAP_ALIGNMENT: u64 = 16;
pub(crate) const CRT_HEAP_PAGE_SIZE: u64 = 4096;
pub(crate) const MAX_CRT_ALLOCATION_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const MAX_CRT_HEAP_BYTES: u64 = 512 * 1024 * 1024;
// Sapphire setup holds more than 4096 small C++ objects concurrently. Keep
// a finite metadata/page-overhead bound while retaining the byte-size limits.
pub(crate) const MAX_CRT_ALLOCATIONS: usize = 65_536;

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
    allocation_hints: RefCell<BTreeMap<(u64, u64), AllocationHint>>,
    live_bytes: u64,
}

struct AllocationHint {
    next: u64,
    // The last tree search proved [next, free_end) unoccupied. Inserts into
    // this interval invalidate it; freeing below next rewinds the cursor.
    free_end: u64,
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
        let backing_alignment = if kind == CrtAllocationKind::EnvironmentStrings {
            CRT_HEAP_PAGE_SIZE
        } else {
            CRT_HEAP_ALIGNMENT
        };
        let backing_size = align_up(requested_size, backing_alignment)?;
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
        let alignment = if allocation.kind == CrtAllocationKind::EnvironmentStrings {
            CRT_HEAP_PAGE_SIZE
        } else {
            CRT_HEAP_ALIGNMENT
        };
        self.first_fit_aligned(range_start, range_end, allocation, alignment)
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
        let mapping_alignment = alignment.max(CRT_HEAP_ALIGNMENT);
        let mut hints = self.allocation_hints.borrow_mut();
        let hint = hints
            .entry((range_start, range_end))
            .or_insert(AllocationHint {
                next: range_start,
                free_end: range_start,
            });
        let hint_in_range = (range_start..range_end).contains(&hint.next);
        let mut candidate = align_up(
            if hint_in_range {
                hint.next
            } else {
                range_start
            },
            mapping_alignment,
        )?;
        if hint_in_range && let Some(end) = candidate.checked_add(allocation.backing_size) {
            if end <= hint.free_end {
                hint.next = end;
                return Ok(candidate);
            }
        }
        // Alignment after an overlapping predecessor can jump past the start
        // of another live block. Keep scanning from the original candidate.
        let scan_start = candidate;
        if let Some((&pointer, existing)) = self.allocations.range(..=candidate).next_back() {
            let existing_end = pointer
                .checked_add(existing.backing_size)
                .ok_or(CrtHeapError::AddressSpaceExhausted)?;
            if existing_end > candidate {
                candidate = align_up(existing_end, mapping_alignment)?;
            }
        }
        if candidate >= range_end {
            return Err(CrtHeapError::AddressSpaceExhausted);
        }
        for (&pointer, existing) in self.allocations.range(scan_start..range_end) {
            let candidate_end = candidate
                .checked_add(allocation.backing_size)
                .ok_or(CrtHeapError::AddressSpaceExhausted)?;
            if candidate_end <= pointer {
                hint.next = candidate_end;
                hint.free_end = pointer;
                return Ok(candidate);
            }
            candidate = candidate.max(align_up(
                pointer
                    .checked_add(existing.backing_size)
                    .ok_or(CrtHeapError::AddressSpaceExhausted)?,
                mapping_alignment,
            )?);
        }
        let selected = candidate
            .checked_add(allocation.backing_size)
            .filter(|end| *end <= range_end)
            .map(|_| candidate)
            .ok_or(CrtHeapError::AddressSpaceExhausted)?;
        hint.next = selected + allocation.backing_size;
        hint.free_end = range_end;
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
        self.occupy_allocation_hints(pointer, allocation.backing_size);
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
        let backing_size = align_up(requested_size, CRT_HEAP_ALIGNMENT)?;
        Ok(CrtAllocation {
            requested_size,
            backing_size,
            kind: CrtAllocationKind::ProcessHeap,
        })
    }

    pub(crate) fn regular_allocation(&self, pointer: u64) -> Result<CrtAllocation, CrtHeapError> {
        let allocation = *self
            .allocations
            .get(&pointer)
            .ok_or(CrtHeapError::ForeignOrFreedPointer)?;
        if allocation.kind != CrtAllocationKind::Regular {
            return Err(CrtHeapError::AllocatorMismatch);
        }
        Ok(allocation)
    }

    pub(crate) fn allocation_containing(&self, pointer: u64) -> Option<(u64, CrtAllocation)> {
        let (&base, &allocation) = self.allocations.range(..=pointer).next_back()?;
        (pointer < base.saturating_add(allocation.requested_size)).then_some((base, allocation))
    }

    pub(crate) fn prepare_regular_reallocation(
        &self,
        pointer: u64,
        requested_size: u64,
    ) -> Result<CrtAllocation, CrtHeapError> {
        let old = self.regular_allocation(pointer)?;
        let requested_size = requested_size.max(1);
        if requested_size > MAX_CRT_ALLOCATION_BYTES {
            return Err(CrtHeapError::AllocationTooLarge);
        }
        let retained_bytes = self.live_bytes - old.requested_size;
        if retained_bytes > MAX_CRT_HEAP_BYTES - requested_size {
            return Err(CrtHeapError::AggregateBudgetExceeded);
        }
        Ok(CrtAllocation {
            requested_size,
            backing_size: align_up(requested_size, CRT_HEAP_PAGE_SIZE)?,
            kind: CrtAllocationKind::Regular,
        })
    }

    pub(crate) fn commit_regular_reallocation(
        &mut self,
        old_pointer: u64,
        new_pointer: u64,
        mut allocation: CrtAllocation,
    ) -> Result<CrtAllocation, CrtHeapError> {
        let old = self.regular_allocation(old_pointer)?;
        if allocation.kind != CrtAllocationKind::Regular {
            return Err(CrtHeapError::AllocatorMismatch);
        }
        if new_pointer == 0 || new_pointer % CRT_HEAP_ALIGNMENT != 0 {
            return Err(CrtHeapError::InvalidPointer);
        }
        if new_pointer != old_pointer && self.allocations.contains_key(&new_pointer) {
            return Err(CrtHeapError::DuplicatePointer);
        }
        if new_pointer == old_pointer {
            allocation.backing_size = old.backing_size;
        }
        self.allocations.remove(&old_pointer);
        self.allocations.insert(new_pointer, allocation);
        self.live_bytes = self.live_bytes - old.requested_size + allocation.requested_size;
        self.occupy_allocation_hints(new_pointer, allocation.backing_size);
        if new_pointer != old_pointer {
            self.rewind_allocation_hints(old_pointer);
        }
        Ok(old)
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
        self.occupy_allocation_hints(new_pointer, allocation.backing_size);
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

    fn occupy_allocation_hints(&mut self, pointer: u64, backing_size: u64) {
        let occupied_end = pointer.saturating_add(backing_size);
        // Direct/native insertions and overlapping search ranges need not
        // correspond to the cursor that selected this allocation.
        for hint in self.allocation_hints.get_mut().values_mut() {
            if pointer < hint.free_end && occupied_end > hint.next {
                hint.free_end = pointer.max(hint.next);
            }
        }
    }

    fn rewind_allocation_hints(&mut self, pointer: u64) {
        for (&(start, end), hint) in self.allocation_hints.get_mut().iter_mut() {
            if (start..end).contains(&pointer) && pointer < hint.next {
                hint.next = pointer;
                hint.free_end = pointer;
            }
        }
    }

    pub(crate) fn allocations(&self) -> impl DoubleEndedIterator<Item = (u64, CrtAllocation)> + '_ {
        self.allocations
            .iter()
            .map(|(&pointer, &allocation)| (pointer, allocation))
    }

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
    fn cached_gap_respects_direct_insertions_and_overlapping_ranges() {
        let mut heap = CrtHeap::default();
        let small = heap.prepare_allocation(16).unwrap();
        assert_eq!(heap.first_fit(0x1000, 0x2000, small), Ok(0x1000));
        heap.insert(0x1000, small).unwrap();
        // Both ranges have a cached free tail. An insertion through either
        // namespace must shorten the other's cached tail too.
        assert_eq!(heap.first_fit(0x1000, 0x3000, small), Ok(0x1010));
        heap.insert(0x1010, small).unwrap();
        let wide = heap.prepare_allocation(0x80).unwrap();
        heap.insert(0x1040, wide).unwrap();
        assert_eq!(heap.first_fit(0x1000, 0x2000, wide), Ok(0x10c0));
        heap.insert(0x10c0, wide).unwrap();
        assert_eq!(heap.first_fit(0x1000, 0x3000, wide), Ok(0x1140));
        // The occupied interval can begin below the current cursor.
        let crossing = heap.prepare_allocation(0x100).unwrap();
        heap.insert(0x1120, crossing).unwrap();
        assert_eq!(heap.first_fit(0x1000, 0x3000, small), Ok(0x1220));
    }

    #[test]
    fn cached_gap_rewinds_on_free_and_moved_realloc() {
        let mut heap = CrtHeap::default();
        let small = heap.prepare_allocation(16).unwrap();
        for pointer in [0x1000, 0x1010, 0x1020] {
            assert_eq!(heap.first_fit(0x1000, 0x2000, small), Ok(pointer));
            heap.insert(pointer, small).unwrap();
        }
        heap.remove(0x1000).unwrap();
        assert_eq!(heap.first_fit(0x1000, 0x2000, small), Ok(0x1000));
        heap.insert(0x1000, small).unwrap();
        let wide = heap.prepare_allocation(32).unwrap();
        assert_eq!(heap.first_fit(0x1000, 0x2000, wide), Ok(0x1030));
        heap.commit_regular_reallocation(0x1000, 0x1100, wide)
            .unwrap();
        assert_eq!(heap.first_fit(0x1000, 0x2000, small), Ok(0x1000));
        assert_eq!(
            heap.first_fit_aligned(0x1000, 0x2000, wide, 0x100),
            Ok(0x1200)
        );

        let process = heap.prepare_process_heap_allocation(16).unwrap();
        heap.insert(0x1300, process).unwrap();
        // Moving process-heap ownership invalidates the cached destination.
        heap.commit_process_heap_reallocation(0x1300, 0x1230, process)
            .unwrap();
        let larger = heap.prepare_allocation(48).unwrap();
        assert_eq!(heap.first_fit(0x1000, 0x2000, larger), Ok(0x1240));
        // An in-place replacement must also invalidate an expanded extent.
        let larger_process = heap
            .prepare_process_heap_reallocation(0x1230, 0x100)
            .unwrap();
        heap.commit_process_heap_reallocation(0x1230, 0x1230, larger_process)
            .unwrap();
        assert_eq!(heap.first_fit(0x1000, 0x2000, small), Ok(0x1330));
    }

    #[test]
    fn cached_gap_handles_alignment_exhaustion_and_uncommitted_searches() {
        let heap = CrtHeap::default();
        let small = heap.prepare_allocation(16).unwrap();
        assert_eq!(heap.first_fit(0x1000, 0x1020, small), Ok(0x1000));
        // Selection still advances the cursor before an allocation commits.
        assert_eq!(heap.first_fit(0x1000, 0x1020, small), Ok(0x1010));
        assert_eq!(heap.first_fit(0x1000, 0x1020, small), Ok(0x1000));
        assert_eq!(
            heap.first_fit_aligned(0x1000, 0x1020, small, 0x2000),
            Err(CrtHeapError::AddressSpaceExhausted)
        );
        assert_eq!(
            heap.first_fit(0x1000, 0x1000, small),
            Err(CrtHeapError::AddressSpaceExhausted)
        );
        assert_eq!(
            heap.first_fit(u64::MAX - 7, u64::MAX, small),
            Err(CrtHeapError::SizeOverflow)
        );
    }

    #[test]
    fn cached_destination_is_invalidated_by_realloc_from_another_range() {
        for process_heap in [false, true] {
            let mut heap = CrtHeap::default();
            let small = if process_heap {
                heap.prepare_process_heap_allocation(16).unwrap()
            } else {
                heap.prepare_allocation(16).unwrap()
            };
            heap.insert(0x1000, small).unwrap();
            assert_eq!(heap.first_fit(0x4000, 0x8000, small), Ok(0x4000));
            heap.insert(0x4000, small).unwrap();
            let large = if process_heap {
                heap.prepare_process_heap_reallocation(0x1000, 0x1000)
                    .unwrap()
            } else {
                heap.prepare_regular_reallocation(0x1000, 0x1000).unwrap()
            };
            if process_heap {
                heap.commit_process_heap_reallocation(0x1000, 0x5000, large)
                    .unwrap();
            } else {
                heap.commit_regular_reallocation(0x1000, 0x5000, large)
                    .unwrap();
            }
            assert_eq!(heap.first_fit(0x4000, 0x8000, large), Ok(0x6000));
            assert_eq!(heap.first_fit(0x1000, 0x2000, small), Ok(0x1000));
        }
    }

    #[test]
    fn aligned_search_does_not_skip_a_block_crossed_by_alignment() {
        let mut heap = CrtHeap::default();
        let small = heap.prepare_allocation(16).unwrap();
        let wide = heap.prepare_allocation(0x100).unwrap();
        heap.insert(0x1000, small).unwrap();
        heap.insert(0x1080, wide).unwrap();
        assert_eq!(
            heap.first_fit_aligned(0x1000, 0x2000, small, 0x100),
            Ok(0x1200)
        );
        heap.insert(0x1200, small).unwrap();
        assert_eq!(heap.first_fit(0x1000, 0x2000, small), Ok(0x1210));
    }

    #[test]
    fn failed_search_preserves_cursor_for_later_free() {
        let mut heap = CrtHeap::default();
        let small = heap.prepare_allocation(16).unwrap();
        for pointer in [0x1000, 0x1020] {
            assert_eq!(
                heap.first_fit_aligned(0x1000, 0x1040, small, 32),
                Ok(pointer)
            );
            heap.insert(pointer, small).unwrap();
        }
        assert_eq!(heap.first_fit(0x1000, 0x1040, small), Ok(0x1030));
        heap.insert(0x1030, small).unwrap();
        let large = heap.prepare_allocation(64).unwrap();
        assert_eq!(
            heap.first_fit(0x1000, 0x1040, large),
            Err(CrtHeapError::AddressSpaceExhausted)
        );
        heap.remove(0x1030).unwrap();
        // Failure must not reset the saved cursor and select the earlier
        // alignment gap at 0x1010 when free rewinds the exhausted cursor.
        assert_eq!(heap.first_fit(0x1000, 0x1040, small), Ok(0x1030));
    }

    #[test]
    fn cached_search_matches_uncached_tree_search_under_churn() {
        // Independent cursor + full-tree oracle exercises the fast path with
        // different sizes, alignments, namespaces, frees, and uncommitted searches.
        let mut heap = CrtHeap::default();
        let mut cursors = BTreeMap::<(u64, u64), u64>::new();
        let mut seed = 123456789u64;
        for step in 0..10_000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            if step % 3 == 0 && !heap.allocations.is_empty() {
                let index = seed as usize % heap.allocations.len();
                let pointer = *heap.allocations.keys().nth(index).unwrap();
                heap.remove(pointer).unwrap();
                for (&(start, end), cursor) in &mut cursors {
                    if (start..end).contains(&pointer) && pointer < *cursor {
                        *cursor = pointer;
                    }
                }
            }
            let start = 0x10000 + (seed % 3) * 0x1000;
            let end = 0x10_0000;
            let size = 1 + (seed >> 32) % 2048;
            let alignment = 16 << ((seed >> 16) % 6);
            let allocation = heap.prepare_allocation(size).unwrap();
            let oracle_before = cursors.get(&(start, end)).copied();
            let mut candidate = align_up(
                *cursors
                    .get(&(start, end))
                    .filter(|&&cursor| (start..end).contains(&cursor))
                    .unwrap_or(&start),
                alignment,
            )
            .unwrap();
            for (&pointer, existing) in &heap.allocations {
                if pointer + existing.backing_size <= candidate {
                    continue;
                }
                if candidate + allocation.backing_size <= pointer {
                    break;
                }
                candidate = align_up(pointer + existing.backing_size, alignment).unwrap();
            }
            let expected = if candidate + allocation.backing_size <= end {
                cursors.insert((start, end), candidate + allocation.backing_size);
                Ok(candidate)
            } else {
                Err(CrtHeapError::AddressSpaceExhausted)
            };
            let hint_before = heap
                .allocation_hints
                .borrow()
                .get(&(start, end))
                .map(|hint| (hint.next, hint.free_end));
            assert_eq!(
                heap.first_fit_aligned(start, end, allocation, alignment),
                expected,
                "step {step}, range {start:#x}..{end:#x}, size {size}, alignment {alignment}, hint {hint_before:?}, oracle {oracle_before:?}"
            );
            if let Ok(pointer) = expected {
                if step % 7 != 0 {
                    heap.insert(pointer, allocation).unwrap();
                }
            }
            for hint in heap.allocation_hints.borrow().values() {
                if hint.next < hint.free_end {
                    let overlap = heap
                        .allocations
                        .range(..hint.free_end)
                        .find(|(pointer, value)| **pointer + value.backing_size > hint.next);
                    assert!(
                        overlap.is_none(),
                        "step {step}: cached {}..{} overlaps {overlap:?}",
                        hint.next,
                        hint.free_end
                    );
                }
            }
        }
        assert_eq!(
            heap.live_bytes(),
            heap.allocations
                .values()
                .map(|a| a.requested_size)
                .sum::<u64>()
        );
    }

    #[test]
    fn zero_size_is_unique_minimum_allocation_and_aligned() {
        let heap = CrtHeap::default();
        let allocation = heap.prepare_allocation(0).unwrap();
        assert_eq!(allocation.requested_size, 1);
        assert_eq!(allocation.backing_size, CRT_HEAP_ALIGNMENT);
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
        for index in 0..(MAX_CRT_HEAP_BYTES / MAX_CRT_ALLOCATION_BYTES) {
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
    fn first_fit_reuses_freed_slot_and_preserves_alignment() {
        let mut heap = CrtHeap::default();
        let allocation = heap.prepare_allocation(17).unwrap();
        let first = heap.first_fit(0x10_0000, 0x20_0000, allocation).unwrap();
        assert_eq!(first % CRT_HEAP_ALIGNMENT, 0);
        heap.insert(first, allocation).unwrap();
        let second = heap.first_fit(0x10_0000, 0x20_0000, allocation).unwrap();
        heap.insert(second, allocation).unwrap();
        assert_eq!(second, first + allocation.backing_size);
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
    fn containing_lookup_finds_interior_bytes_without_crossing_requested_end() {
        let mut heap = CrtHeap::default();
        let first = heap.prepare_allocation(17).unwrap();
        let second = heap.prepare_allocation(9).unwrap();
        heap.insert(0x1000, first).unwrap();
        heap.insert(0x2000, second).unwrap();
        assert_eq!(heap.allocation_containing(0x1000), Some((0x1000, first)));
        assert_eq!(heap.allocation_containing(0x1010), Some((0x1000, first)));
        assert_eq!(heap.allocation_containing(0x1011), None);
        assert_eq!(heap.allocation_containing(0x1fff), None);
        assert_eq!(heap.allocation_containing(0x2008), Some((0x2000, second)));
        assert_eq!(heap.allocation_containing(0x2009), None);
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
