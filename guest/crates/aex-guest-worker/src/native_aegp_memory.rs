//! Platform-neutral state machine behind the native x86_64 AEGP Memory Suite v1 callbacks.

use std::cell::Cell;
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

struct ActiveContext {
    memory: *mut NativeAegpMemory,
    arena_next: *mut u64,
    arena_end: u64,
}

thread_local! {
    static ACTIVE_CONTEXT: Cell<*mut ActiveContext> = const { Cell::new(ptr::null_mut()) };
}

pub(crate) struct NativeAegpMemoryContextGuard {
    _context: Box<ActiveContext>,
    previous: *mut ActiveContext,
}

impl Drop for NativeAegpMemoryContextGuard {
    fn drop(&mut self) {
        ACTIVE_CONTEXT.with(|slot| slot.set(self.previous));
    }
}

pub(crate) fn activate_native_aegp_memory_context(
    memory: &mut NativeAegpMemory,
    arena_next: &mut u64,
    arena_end: u64,
) -> NativeAegpMemoryContextGuard {
    let mut context = Box::new(ActiveContext {
        memory,
        arena_next,
        arena_end,
    });
    let previous = ACTIVE_CONTEXT.with(|slot| slot.replace(context.as_mut()));
    NativeAegpMemoryContextGuard {
        _context: context,
        previous,
    }
}

pub(crate) fn with_native_aegp_memory_context<T>(
    memory: &mut NativeAegpMemory,
    arena_next: &mut u64,
    arena_end: u64,
    operation: impl FnOnce() -> T,
) -> T {
    let _context = activate_native_aegp_memory_context(memory, arena_next, arena_end);
    operation()
}

fn with_active_context<T>(operation: impl FnOnce(&mut ActiveContext) -> T) -> Option<T> {
    ACTIVE_CONTEXT.with(|slot| {
        let context = slot.get();
        if context.is_null() {
            None
        } else {
            Some(unsafe { operation(&mut *context) })
        }
    })
}

#[cfg(test)]
pub(crate) fn active_arena_next() -> Option<u64> {
    with_active_context(|context| unsafe { *context.arena_next })
}

macro_rules! callback_address {
    ($callback:expr) => {
        $callback as *const () as usize as u64
    };
}

pub(crate) fn native_aegp_memory_callbacks() -> [u64; 8] {
    [
        callback_address!(new_aegp_mem_handle),
        callback_address!(free_aegp_mem_handle),
        callback_address!(lock_aegp_mem_handle),
        callback_address!(unlock_aegp_mem_handle),
        callback_address!(get_aegp_mem_handle_size),
        callback_address!(resize_aegp_mem_handle),
        callback_address!(unsupported_aegp_memory_slot),
        callback_address!(unsupported_aegp_memory_slot),
    ]
}

impl NativeAegpMemory {
    fn valid_size(size: u64) -> Option<u64> {
        (size <= i32::MAX as u64 && size <= MAX_AEGP_MEMORY_BYTES).then_some(size)
    }

    fn checked_live_bytes_after(&self, replaced_size: u64, size: u64) -> Option<u64> {
        let live_bytes = self
            .handles
            .values()
            .try_fold(0u64, |total, record| total.checked_add(record.size))?;
        let total = live_bytes.checked_sub(replaced_size)?.checked_add(size)?;
        (total <= MAX_AEGP_MEMORY_BYTES).then_some(total)
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
        if self.checked_live_bytes_after(0, size).is_none() {
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
        if self.checked_live_bytes_after(old.size, size).is_none() {
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

pub(crate) unsafe extern "win64" fn new_aegp_mem_handle(
    plugin_id: u64,
    what: u64,
    size: u64,
    flags: u64,
    output: u64,
    _: u64,
) -> u64 {
    with_active_context(|context| unsafe {
        (*context.memory).new_handle(
            &mut *context.arena_next,
            context.arena_end,
            plugin_id,
            what,
            size,
            flags,
            output,
        )
    })
    .unwrap_or(4)
}

pub(crate) unsafe extern "win64" fn free_aegp_mem_handle(
    handle: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    with_active_context(|context| unsafe { (*context.memory).free_handle(handle) }).unwrap_or(4)
}

pub(crate) unsafe extern "win64" fn lock_aegp_mem_handle(
    handle: u64,
    output: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    with_active_context(|context| unsafe { (*context.memory).lock_handle(handle, output) })
        .unwrap_or(4)
}

pub(crate) unsafe extern "win64" fn unlock_aegp_mem_handle(
    handle: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    with_active_context(|context| unsafe { (*context.memory).unlock_handle(handle) }).unwrap_or(4)
}

pub(crate) unsafe extern "win64" fn get_aegp_mem_handle_size(
    handle: u64,
    output: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    with_active_context(|context| unsafe { (*context.memory).handle_size(handle, output) })
        .unwrap_or(4)
}

pub(crate) unsafe extern "win64" fn resize_aegp_mem_handle(
    what: u64,
    size: u64,
    handle: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    with_active_context(|context| unsafe {
        (*context.memory).resize_handle(
            &mut *context.arena_next,
            context.arena_end,
            what,
            size,
            handle,
        )
    })
    .unwrap_or(4)
}

pub(crate) unsafe extern "win64" fn unsupported_aegp_memory_slot(
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    4
}

#[cfg(all(test, target_arch = "x86_64", target_os = "windows"))]
mod windows_tests {
    use super::*;
    use std::ffi::CString;

    type Win64Function = unsafe extern "win64" fn(u64, u64, u64, u64, u64, u64) -> u64;

    fn callback(table: &[u64; 8], slot: usize) -> Win64Function {
        unsafe { std::mem::transmute(table[slot] as usize) }
    }

    fn active_memory_state_snapshot() -> (u64, u64, Vec<(u64, u64, u64, u32, u64)>, Vec<(u64, u64)>)
    {
        let arena_next = active_arena_next().unwrap();
        let (next_handle, mut handles, free) = with_active_context(|context| unsafe {
            let memory = &*context.memory;
            let handles = memory
                .handles
                .iter()
                .map(|(&handle, record)| {
                    (handle, record.data, record.size, record.locks, record.end)
                })
                .collect::<Vec<_>>();
            let free = memory
                .free
                .iter()
                .map(|block| (block.data, block.end))
                .collect::<Vec<_>>();
            (memory.next_handle, handles, free)
        })
        .unwrap();
        handles.sort_by_key(|record| record.0);
        (arena_next, next_handle, handles, free)
    }

    #[test]
    fn windows_native_x86_64_carrier_callback_lifecycle() {
        let mut arena = vec![0u8; 0x10000];
        let mut arena_next = arena.as_mut_ptr() as u64;
        let arena_end = arena_next + arena.len() as u64;
        let mut memory = NativeAegpMemory::default();
        let label = CString::new("windows_native_carrier").unwrap();
        let table = native_aegp_memory_callbacks();
        let new_handle = callback(&table, 0);
        let free_handle = callback(&table, 1);
        let lock_handle = callback(&table, 2);
        let unlock_handle = callback(&table, 3);
        let handle_size = callback(&table, 4);
        let resize_handle = callback(&table, 5);

        let mut missing_context_output = u64::MAX;
        assert_eq!(
            unsafe {
                new_handle(
                    1,
                    label.as_ptr() as u64,
                    4,
                    0,
                    (&mut missing_context_output as *mut u64) as u64,
                    0,
                )
            },
            4
        );
        assert_eq!(missing_context_output, u64::MAX);

        with_native_aegp_memory_context(&mut memory, &mut arena_next, arena_end, || {
            let mut invalid_handle = u64::MAX;
            assert_eq!(
                unsafe {
                    new_handle(
                        2,
                        label.as_ptr() as u64,
                        4,
                        0,
                        (&mut invalid_handle as *mut u64) as u64,
                        0,
                    )
                },
                4
            );
            assert_eq!(invalid_handle, 0);

            let mut handle = 0u64;
            assert_eq!(
                unsafe {
                    new_handle(
                        1,
                        label.as_ptr() as u64,
                        4,
                        0,
                        (&mut handle as *mut u64) as u64,
                        0,
                    )
                },
                0
            );
            let mut data = 0u64;
            assert_eq!(
                unsafe { lock_handle(handle, (&mut data as *mut u64) as u64, 0, 0, 0, 0) },
                0
            );
            assert_eq!(data % 16, 0);
            unsafe {
                *(data as *mut u32) = 0x1122_3344;
            }
            assert_eq!(unsafe { free_handle(handle, 0, 0, 0, 0, 0) }, 4);
            assert_eq!(
                unsafe { resize_handle(label.as_ptr() as u64, 8, handle, 0, 0, 0) },
                4
            );
            assert_eq!(unsafe { unlock_handle(handle, 0, 0, 0, 0, 0) }, 0);

            let mut size = 0u32;
            assert_eq!(
                unsafe { handle_size(handle, (&mut size as *mut u32) as u64, 0, 0, 0, 0) },
                0
            );
            assert_eq!(size, 4);
            assert_eq!(
                unsafe { resize_handle(label.as_ptr() as u64, 8, handle, 0, 0, 0) },
                0
            );
            assert_eq!(
                unsafe { lock_handle(handle, (&mut data as *mut u64) as u64, 0, 0, 0, 0) },
                0
            );
            assert_eq!(unsafe { *(data as *const u32) }, 0x1122_3344);
            assert_eq!(unsafe { *((data + 4) as *const u32) }, 0);
            assert_eq!(unsafe { unlock_handle(handle, 0, 0, 0, 0, 0) }, 0);
            for _ in 0..(MAX_AEGP_MEMORY_HANDLES * 2) {
                assert_eq!(
                    unsafe { resize_handle(label.as_ptr() as u64, 128, handle, 0, 0, 0) },
                    0
                );
                assert_eq!(
                    unsafe { resize_handle(label.as_ptr() as u64, 32, handle, 0, 0, 0) },
                    0
                );
            }
            assert_eq!(unsafe { free_handle(handle, 0, 0, 0, 0, 0) }, 0);
            assert_eq!(unsafe { free_handle(handle, 0, 0, 0, 0, 0) }, 4);
            assert_eq!(
                unsafe { handle_size(handle, (&mut size as *mut u32) as u64, 0, 0, 0, 0) },
                4
            );

            let mut reuse_high_water = 0;
            for cycle in 0..(MAX_AEGP_MEMORY_HANDLES * 2) {
                let mut recycled = 0u64;
                assert_eq!(
                    unsafe {
                        new_handle(
                            1,
                            label.as_ptr() as u64,
                            32,
                            0,
                            (&mut recycled as *mut u64) as u64,
                            0,
                        )
                    },
                    0
                );
                assert_eq!(unsafe { free_handle(recycled, 0, 0, 0, 0, 0) }, 0);
                if cycle == 0 {
                    reuse_high_water = active_arena_next().unwrap();
                } else {
                    assert_eq!(active_arena_next().unwrap(), reuse_high_water);
                }
            }

            assert_eq!(unsafe { callback(&table, 6)(0, 0, 0, 0, 0, 0) }, 4);
            assert_eq!(unsafe { callback(&table, 7)(0, 0, 0, 0, 0, 0) }, 4);

            let outer_arena_next = active_arena_next().unwrap();
            let mut nested_arena = vec![0u8; 0x100];
            let mut nested_arena_next = nested_arena.as_mut_ptr() as u64;
            let nested_arena_end = nested_arena_next + nested_arena.len() as u64;
            let mut nested_memory = NativeAegpMemory::default();
            with_native_aegp_memory_context(
                &mut nested_memory,
                &mut nested_arena_next,
                nested_arena_end,
                || {
                    let mut nested_handle = 0;
                    assert_eq!(
                        unsafe {
                            new_handle(
                                1,
                                label.as_ptr() as u64,
                                4,
                                0,
                                (&mut nested_handle as *mut u64) as u64,
                                0,
                            )
                        },
                        0
                    );
                    assert_eq!(unsafe { free_handle(nested_handle, 0, 0, 0, 0, 0) }, 0);
                },
            );
            assert_eq!(active_arena_next(), Some(outer_arena_next));
            let mut restored_outer_handle = 0;
            assert_eq!(
                unsafe {
                    new_handle(
                        1,
                        label.as_ptr() as u64,
                        4,
                        0,
                        (&mut restored_outer_handle as *mut u64) as u64,
                        0,
                    )
                },
                0
            );
            assert_eq!(
                unsafe { free_handle(restored_outer_handle, 0, 0, 0, 0, 0) },
                0
            );
        });

        assert_eq!(active_arena_next(), None);
        let mut post_scope_handle = u64::MAX;
        assert_eq!(
            unsafe {
                new_handle(
                    1,
                    label.as_ptr() as u64,
                    4,
                    0,
                    (&mut post_scope_handle as *mut u64) as u64,
                    0,
                )
            },
            4
        );
        assert_eq!(post_scope_handle, u64::MAX);
    }

    #[test]
    fn windows_native_callback_table_enforces_aggregate_live_byte_budget_atomically() {
        const MIB: u64 = 1024 * 1024;

        let mut arena = vec![0u8; (MAX_AEGP_MEMORY_BYTES * 2 + 16) as usize];
        let mut arena_next = arena.as_mut_ptr() as u64;
        let arena_end = arena_next + arena.len() as u64;
        let mut memory = NativeAegpMemory::default();
        let label = CString::new("native_memory_budget").unwrap();
        let table = native_aegp_memory_callbacks();
        let new_handle = callback(&table, 0);
        let free_handle = callback(&table, 1);
        let lock_handle = callback(&table, 2);
        let unlock_handle = callback(&table, 3);
        let resize_handle = callback(&table, 5);

        with_native_aegp_memory_context(&mut memory, &mut arena_next, arena_end, || {
            let mut nine_mib_handle = u64::MAX;
            assert_eq!(
                unsafe {
                    new_handle(
                        1,
                        label.as_ptr() as u64,
                        9 * MIB,
                        0,
                        (&mut nine_mib_handle as *mut u64) as u64,
                        0,
                    )
                },
                0
            );
            let mut nine_mib_data = 0u64;
            assert_eq!(
                unsafe {
                    lock_handle(
                        nine_mib_handle,
                        (&mut nine_mib_data as *mut u64) as u64,
                        0,
                        0,
                        0,
                        0,
                    )
                },
                0
            );
            unsafe {
                *(nine_mib_data as *mut u32) = 0x5142_3324;
            }
            assert_eq!(unsafe { unlock_handle(nine_mib_handle, 0, 0, 0, 0, 0) }, 0);

            let before_rejected_new = active_memory_state_snapshot();
            let mut rejected_handle = u64::MAX;
            assert_eq!(
                unsafe {
                    new_handle(
                        1,
                        label.as_ptr() as u64,
                        9 * MIB,
                        0,
                        (&mut rejected_handle as *mut u64) as u64,
                        0,
                    )
                },
                4
            );
            assert_eq!(rejected_handle, 0);
            assert_eq!(active_memory_state_snapshot(), before_rejected_new);
            assert_eq!(unsafe { *(nine_mib_data as *const u32) }, 0x5142_3324);
            assert_eq!(unsafe { free_handle(nine_mib_handle, 0, 0, 0, 0, 0) }, 0);

            let mut eight_mib_a = 0;
            let mut eight_mib_b = 0;
            for output in [&mut eight_mib_a, &mut eight_mib_b] {
                assert_eq!(
                    unsafe {
                        new_handle(
                            1,
                            label.as_ptr() as u64,
                            8 * MIB,
                            0,
                            (output as *mut u64) as u64,
                            0,
                        )
                    },
                    0
                );
            }
            let mut eight_mib_a_data = 0u64;
            assert_eq!(
                unsafe {
                    lock_handle(
                        eight_mib_a,
                        (&mut eight_mib_a_data as *mut u64) as u64,
                        0,
                        0,
                        0,
                        0,
                    )
                },
                0
            );
            unsafe {
                *(eight_mib_a_data as *mut u32) = 0xa1b2_c3d4;
            }
            assert_eq!(unsafe { unlock_handle(eight_mib_a, 0, 0, 0, 0, 0) }, 0);

            let before_one_byte_rejection = active_memory_state_snapshot();
            rejected_handle = u64::MAX;
            assert_eq!(
                unsafe {
                    new_handle(
                        1,
                        label.as_ptr() as u64,
                        1,
                        0,
                        (&mut rejected_handle as *mut u64) as u64,
                        0,
                    )
                },
                4
            );
            assert_eq!(rejected_handle, 0);
            assert_eq!(active_memory_state_snapshot(), before_one_byte_rejection);

            assert_eq!(unsafe { free_handle(eight_mib_b, 0, 0, 0, 0, 0) }, 0);
            let mut replacement_eight_mib = 0;
            assert_eq!(
                unsafe {
                    new_handle(
                        1,
                        label.as_ptr() as u64,
                        8 * MIB,
                        0,
                        (&mut replacement_eight_mib as *mut u64) as u64,
                        0,
                    )
                },
                0
            );
            assert_eq!(
                unsafe { resize_handle(label.as_ptr() as u64, 8 * MIB, eight_mib_a, 0, 0, 0) },
                0
            );

            let before_rejected_resize = active_memory_state_snapshot();
            assert_eq!(
                unsafe { resize_handle(label.as_ptr() as u64, 9 * MIB, eight_mib_a, 0, 0, 0) },
                4
            );
            assert_eq!(active_memory_state_snapshot(), before_rejected_resize);
            let mut data_after_rejection = 0u64;
            assert_eq!(
                unsafe {
                    lock_handle(
                        eight_mib_a,
                        (&mut data_after_rejection as *mut u64) as u64,
                        0,
                        0,
                        0,
                        0,
                    )
                },
                0
            );
            assert_eq!(data_after_rejection, eight_mib_a_data);
            assert_eq!(
                unsafe { *(data_after_rejection as *const u32) },
                0xa1b2_c3d4
            );
            assert_eq!(unsafe { unlock_handle(eight_mib_a, 0, 0, 0, 0, 0) }, 0);

            assert_eq!(
                unsafe { resize_handle(label.as_ptr() as u64, 7 * MIB, eight_mib_a, 0, 0, 0) },
                0
            );
            let mut recovered_one_mib = 0;
            assert_eq!(
                unsafe {
                    new_handle(
                        1,
                        label.as_ptr() as u64,
                        MIB,
                        0,
                        (&mut recovered_one_mib as *mut u64) as u64,
                        0,
                    )
                },
                0
            );

            for handle in [eight_mib_a, replacement_eight_mib, recovered_one_mib] {
                assert_eq!(unsafe { free_handle(handle, 0, 0, 0, 0, 0) }, 0);
            }
            assert!(active_memory_state_snapshot().2.is_empty());
        });
        assert_eq!(active_arena_next(), None);
    }
}
