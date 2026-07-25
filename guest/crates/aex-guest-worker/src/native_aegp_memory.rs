//! Platform-neutral state machine behind the native x86_64 AEGP Memory Suite v1 callbacks.

use std::collections::HashMap;
use std::ptr;

const MAX_AEGP_MEMORY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_AEGP_MEMORY_HANDLES: usize = 256;

#[derive(Clone, Debug)]
struct Handle {
    data: u64,
    size: u64,
    locks: u32,
    end: u64,
}

#[derive(Clone, Copy, Debug)]
struct Block {
    data: u64,
    end: u64,
}

#[derive(Default)]
pub(crate) struct NativeAegpMemory {
    handles: HashMap<u64, Handle>,
    free: Vec<Block>,
    next_handle: u64,
}

impl NativeAegpMemory {
    fn valid_size(size: u64) -> Option<u64> {
        (size <= i32::MAX as u64 && size <= MAX_AEGP_MEMORY_BYTES).then_some(size)
    }

    fn capacity(size: u64) -> Option<u64> {
        size.max(1).checked_add(15).map(|size| size & !15)
    }

    fn select_block(
        &self,
        size: u64,
        arena_next: u64,
        arena_end: u64,
    ) -> Option<(Block, Option<usize>, Option<Block>)> {
        let capacity = Self::capacity(size)?;
        if let Some((index, block)) = self
            .free
            .iter()
            .copied()
            .enumerate()
            .find(|(_, block)| block.end - block.data >= capacity)
        {
            let end = block.data.checked_add(capacity)?;
            let remainder = (end < block.end).then_some(Block {
                data: end,
                end: block.end,
            });
            return Some((
                Block {
                    data: block.data,
                    end,
                },
                Some(index),
                remainder,
            ));
        }
        let data = (arena_next + 15) & !15;
        let end = data.checked_add(capacity)?;
        (end <= arena_end).then_some((Block { data, end }, None, None))
    }

    fn commit_block(
        &mut self,
        block: Block,
        free_index: Option<usize>,
        remainder: Option<Block>,
        arena_next: &mut u64,
    ) {
        if let Some(index) = free_index {
            self.free.remove(index);
            if let Some(remainder) = remainder {
                self.free.push(remainder);
            }
        } else {
            *arena_next = block.end;
        }
    }

    fn reclaim_block(&mut self, block: Block) {
        self.free.push(block);
        self.free.sort_by_key(|block| block.data);
        let mut merged: Vec<Block> = Vec::with_capacity(self.free.len());
        for block in self.free.drain(..) {
            if let Some(last) = merged.last_mut()
                && block.data <= last.end
            {
                last.end = last.end.max(block.end);
            } else {
                merged.push(block);
            }
        }
        self.free = merged;
    }

    pub(crate) unsafe fn new_handle(
        &mut self,
        arena_next: &mut u64,
        arena_end: u64,
        plugin_id: u64,
        what: u64,
        size: u64,
        flags: u64,
        output: u64,
    ) -> u64 {
        if output != 0 {
            unsafe {
                *(output as *mut u64) = 0;
            }
        }
        let Some(size) = Self::valid_size(size) else {
            return 4;
        };
        if plugin_id != 1
            || what == 0
            || output == 0
            || flags > u32::MAX as u64
            || flags & !3 != 0
            || self.handles.len() >= MAX_AEGP_MEMORY_HANDLES
        {
            return 4;
        }
        let handle = self.next_handle.max(8);
        let Some(next_handle) = handle.checked_add(8).filter(|next| *next != 0) else {
            return 4;
        };
        let Some((block, free_index, remainder)) = self.select_block(size, *arena_next, arena_end)
        else {
            return 4;
        };
        unsafe {
            ptr::write_bytes(block.data as *mut u8, 0, size as usize);
            *(output as *mut u64) = handle;
        }
        self.commit_block(block, free_index, remainder, arena_next);
        self.next_handle = next_handle;
        self.handles.insert(
            handle,
            Handle {
                data: block.data,
                size,
                locks: 0,
                end: block.end,
            },
        );
        0
    }

    pub(crate) fn free_handle(&mut self, handle: u64) -> u64 {
        if self
            .handles
            .get(&handle)
            .is_some_and(|record| record.locks == 0)
        {
            let record = self.handles.remove(&handle).expect("checked handle exists");
            self.reclaim_block(Block {
                data: record.data,
                end: record.end,
            });
            0
        } else {
            4
        }
    }

    pub(crate) unsafe fn lock_handle(&mut self, handle: u64, output: u64) -> u64 {
        if output == 0 {
            return 4;
        }
        let Some(record) = self.handles.get_mut(&handle) else {
            return 4;
        };
        if record.locks == u32::MAX {
            return 4;
        }
        unsafe {
            *(output as *mut u64) = record.data;
        }
        record.locks += 1;
        0
    }

    pub(crate) fn unlock_handle(&mut self, handle: u64) -> u64 {
        let Some(record) = self.handles.get_mut(&handle) else {
            return 4;
        };
        if record.locks == 0 {
            return 4;
        }
        record.locks -= 1;
        0
    }

    pub(crate) unsafe fn handle_size(&self, handle: u64, output: u64) -> u64 {
        if output == 0 {
            return 4;
        }
        let Some(record) = self.handles.get(&handle) else {
            return 4;
        };
        unsafe {
            *(output as *mut u32) = record.size as u32;
        }
        0
    }

    pub(crate) unsafe fn resize_handle(
        &mut self,
        arena_next: &mut u64,
        arena_end: u64,
        what: u64,
        size: u64,
        handle: u64,
    ) -> u64 {
        let Some(size) = Self::valid_size(size) else {
            return 4;
        };
        if what == 0 {
            return 4;
        }
        let Some(old) = self.handles.get(&handle).cloned() else {
            return 4;
        };
        if old.locks != 0 {
            return 4;
        }
        let old_capacity = old.end - old.data;
        let Some(new_capacity) = Self::capacity(size) else {
            return 4;
        };
        if new_capacity <= old_capacity {
            unsafe {
                if size > old.size {
                    ptr::write_bytes(
                        (old.data + old.size) as *mut u8,
                        0,
                        (size - old.size) as usize,
                    );
                }
            }
            let new_end = old.data + new_capacity;
            let Some(record) = self.handles.get_mut(&handle) else {
                return 4;
            };
            record.size = size;
            record.end = new_end;
            if new_end < old.end {
                self.reclaim_block(Block {
                    data: new_end,
                    end: old.end,
                });
            }
            return 0;
        }
        let Some((block, free_index, remainder)) = self.select_block(size, *arena_next, arena_end)
        else {
            return 4;
        };
        unsafe {
            ptr::write_bytes(block.data as *mut u8, 0, size as usize);
            ptr::copy_nonoverlapping(
                old.data as *const u8,
                block.data as *mut u8,
                old.size.min(size) as usize,
            );
        }
        self.commit_block(block, free_index, remainder, arena_next);
        let Some(record) = self.handles.get_mut(&handle) else {
            return 4;
        };
        record.data = block.data;
        record.size = size;
        record.end = block.end;
        self.reclaim_block(Block {
            data: old.data,
            end: old.end,
        });
        0
    }
}

#[cfg(all(test, target_arch = "x86_64", target_os = "windows"))]
mod windows_tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn windows_native_x86_64_carrier_memory_lifecycle() {
        let mut arena = vec![0u8; 0x10000];
        let mut arena_next = arena.as_mut_ptr() as u64;
        let arena_end = arena_next + arena.len() as u64;
        let mut memory = NativeAegpMemory::default();
        let label = CString::new("windows_native_carrier").unwrap();

        let mut handle = 0u64;
        assert_eq!(
            unsafe {
                memory.new_handle(
                    &mut arena_next,
                    arena_end,
                    1,
                    label.as_ptr() as u64,
                    4,
                    0,
                    (&mut handle as *mut u64) as u64,
                )
            },
            0
        );
        let mut data = 0u64;
        assert_eq!(
            unsafe { memory.lock_handle(handle, (&mut data as *mut u64) as u64) },
            0
        );
        assert_eq!(data % 16, 0);
        unsafe {
            *(data as *mut u32) = 0x1122_3344;
        }
        assert_eq!(memory.free_handle(handle), 4);
        assert_eq!(
            unsafe {
                memory.resize_handle(&mut arena_next, arena_end, label.as_ptr() as u64, 8, handle)
            },
            4
        );
        assert_eq!(memory.unlock_handle(handle), 0);

        let mut size = 0u32;
        assert_eq!(
            unsafe { memory.handle_size(handle, (&mut size as *mut u32) as u64) },
            0
        );
        assert_eq!(size, 4);
        assert_eq!(
            unsafe {
                memory.resize_handle(&mut arena_next, arena_end, label.as_ptr() as u64, 8, handle)
            },
            0
        );
        assert_eq!(
            unsafe { memory.lock_handle(handle, (&mut data as *mut u64) as u64) },
            0
        );
        assert_eq!(unsafe { *(data as *const u32) }, 0x1122_3344);
        assert_eq!(unsafe { *((data + 4) as *const u32) }, 0);
        assert_eq!(memory.unlock_handle(handle), 0);
        for _ in 0..(MAX_AEGP_MEMORY_HANDLES * 2) {
            assert_eq!(
                unsafe {
                    memory.resize_handle(
                        &mut arena_next,
                        arena_end,
                        label.as_ptr() as u64,
                        128,
                        handle,
                    )
                },
                0
            );
            assert_eq!(
                unsafe {
                    memory.resize_handle(
                        &mut arena_next,
                        arena_end,
                        label.as_ptr() as u64,
                        32,
                        handle,
                    )
                },
                0
            );
        }
        assert_eq!(memory.free_handle(handle), 0);
        assert_eq!(memory.free_handle(handle), 4);
        assert_eq!(
            unsafe { memory.handle_size(handle, (&mut size as *mut u32) as u64) },
            4
        );

        let mut reuse_high_water = 0;
        for cycle in 0..(MAX_AEGP_MEMORY_HANDLES * 2) {
            let mut recycled = 0u64;
            assert_eq!(
                unsafe {
                    memory.new_handle(
                        &mut arena_next,
                        arena_end,
                        1,
                        label.as_ptr() as u64,
                        32,
                        0,
                        (&mut recycled as *mut u64) as u64,
                    )
                },
                0
            );
            assert_eq!(memory.free_handle(recycled), 0);
            if cycle == 0 {
                reuse_high_water = arena_next;
            } else {
                assert_eq!(arena_next, reuse_high_water);
            }
        }
    }
}
