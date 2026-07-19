//! issue #98 項目5 spike fixture: 継承 handle だけを頼りに、restricted token +
//! Job Object (ProcessMemoryLimit) 下から無名 file mapping を MapViewOfFile し、
//! 共有 view への書き込み・全ページ touch・イベント往復を行い、自プロセスの
//! メモリカウンタを共有メモリ経由で親に報告する。
//!
//! 契約 (render_session_shm_spike.rs と対):
//! - 環境変数 AEXCOMPAT_SPIKE_* で handle 番号と構成を受ける。
//! - section レイアウト: [0..64) 親パターン、[size-64..size) 親パターン、
//!   offset 128 に ACK マーカー、offset 4096 に u32 LE 長 + JSON レポート。
//! - exit code は失敗ビットマップ (0 = 全成功)。

#[cfg(windows)]
mod probe {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Memory::{
        MapViewOfFile, VirtualAlloc, VirtualFree, FILE_MAP_ALL_ACCESS, MEM_COMMIT, MEM_RELEASE,
        MEM_RESERVE, PAGE_READWRITE,
    };
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, SetEvent, WaitForSingleObject,
    };

    pub const FAIL_ENV: u32 = 1 << 0;
    pub const FAIL_MAP: u32 = 1 << 1;
    pub const FAIL_PATTERN: u32 = 1 << 2;
    pub const FAIL_COUNTERS: u32 = 1 << 3;
    pub const FAIL_PINGPONG: u32 = 1 << 4;
    pub const FAIL_CAP_NOT_ENFORCED: u32 = 1 << 5;
    pub const FAIL_REPORT: u32 = 1 << 6;

    const ACK_OFFSET: usize = 128;
    const ACK: &[u8] = b"AEXCOMPAT_SPIKE_ACK1";
    const REPORT_OFFSET: usize = 4096;
    const PAGE: usize = 4096;
    const PATTERN_BYTES: usize = 64;

    fn env_usize(name: &str) -> Option<usize> {
        std::env::var(name).ok()?.parse::<usize>().ok()
    }

    fn head_pattern(index: usize) -> u8 {
        (index as u8).wrapping_mul(31).wrapping_add(7)
    }

    fn tail_pattern(index: usize) -> u8 {
        (index as u8).wrapping_mul(59).wrapping_add(3)
    }

    /// (PagefileUsage, PeakPagefileUsage, WorkingSetSize)。PagefileUsage が
    /// プロセスの private commit で、Job の ProcessMemoryLimit が制限する量。
    fn counters() -> Option<(u64, u64, u64)> {
        let mut info: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            K32GetProcessMemoryInfo(
                GetCurrentProcess(),
                &mut info,
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            )
        };
        (ok != 0).then_some((
            info.PagefileUsage as u64,
            info.PeakPagefileUsage as u64,
            info.WorkingSetSize as u64,
        ))
    }

    pub fn run() -> u32 {
        let mut failures = 0u32;
        let (Some(section), Some(req), Some(rsp), Some(bytes), Some(rounds), Some(private_probe)) = (
            env_usize("AEXCOMPAT_SPIKE_SECTION_HANDLE"),
            env_usize("AEXCOMPAT_SPIKE_REQ_EVENT"),
            env_usize("AEXCOMPAT_SPIKE_RSP_EVENT"),
            env_usize("AEXCOMPAT_SPIKE_SECTION_BYTES"),
            env_usize("AEXCOMPAT_SPIKE_PINGPONG_ROUNDS"),
            env_usize("AEXCOMPAT_SPIKE_PRIVATE_PROBE_BYTES"),
        ) else {
            return FAIL_ENV;
        };
        let section = section as HANDLE;
        let req = req as HANDLE;
        let rsp = rsp as HANDLE;

        let before_map = counters();
        let view = unsafe { MapViewOfFile(section, FILE_MAP_ALL_ACCESS, 0, 0, 0) };
        let map_error = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        if view.Value.is_null() {
            // view 無しではレポートも ACK も書けないので即終了。親は exit code
            // と GetLastError 相当の情報なしで FAIL_MAP を観測する。
            return failures | FAIL_MAP | ((map_error as u32) << 16);
        }
        let base = view.Value as *mut u8;

        for index in 0..PATTERN_BYTES {
            if unsafe { base.add(index).read_volatile() } != head_pattern(index) {
                failures |= FAIL_PATTERN;
                break;
            }
        }
        for index in 0..PATTERN_BYTES {
            let offset = bytes - PATTERN_BYTES + index;
            if unsafe { base.add(offset).read_volatile() } != tail_pattern(index) {
                failures |= FAIL_PATTERN;
                break;
            }
        }
        let after_map = counters();

        unsafe { std::ptr::copy_nonoverlapping(ACK.as_ptr(), base.add(ACK_OFFSET), ACK.len()) };
        if unsafe { SetEvent(rsp) } == 0 {
            failures |= FAIL_PINGPONG;
        }
        for _ in 0..rounds {
            if failures & FAIL_PINGPONG != 0 {
                break;
            }
            if unsafe { WaitForSingleObject(req, 10_000) } != WAIT_OBJECT_0 {
                failures |= FAIL_PINGPONG;
                break;
            }
            if unsafe { SetEvent(rsp) } == 0 {
                failures |= FAIL_PINGPONG;
            }
        }

        let mut offset = 0usize;
        while offset < bytes {
            unsafe { base.add(offset).write_volatile(0xA5) };
            offset += PAGE;
        }
        let after_touch = counters();
        if before_map.is_none() || after_map.is_none() || after_touch.is_none() {
            failures |= FAIL_COUNTERS;
        }

        // Job の commit 上限がこのプロセスに実際に効いている証拠: 上限を超える
        // private commit は失敗しなければならない。成功したら上限が効いて
        // いないので、共有 view の「課金されない」観測も証拠力を失う。
        let alloc = unsafe {
            VirtualAlloc(
                null_mut(),
                private_probe,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        let alloc_error = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        let alloc_failed = alloc.is_null();
        if !alloc_failed {
            unsafe { VirtualFree(alloc, 0, MEM_RELEASE) };
            failures |= FAIL_CAP_NOT_ENFORCED;
        }

        let zero = (0u64, 0u64, 0u64);
        let before_map = before_map.unwrap_or(zero);
        let after_map = after_map.unwrap_or(zero);
        let after_touch = after_touch.unwrap_or(zero);
        let report = format!(
            concat!(
                "{{\"pagefile_before_map\":{},\"pagefile_after_map\":{},",
                "\"pagefile_after_touch\":{},\"peak_pagefile_after_touch\":{},",
                "\"working_set_after_touch\":{},\"private_alloc_bytes\":{},",
                "\"private_alloc_failed\":{},\"private_alloc_error\":{},",
                "\"map_error\":{}}}"
            ),
            before_map.0,
            after_map.0,
            after_touch.0,
            after_touch.1,
            after_touch.2,
            private_probe,
            alloc_failed,
            alloc_error,
            map_error,
        );
        let payload = report.as_bytes();
        if REPORT_OFFSET + 4 + payload.len() > bytes {
            failures |= FAIL_REPORT;
        } else {
            unsafe {
                let length = (payload.len() as u32).to_le_bytes();
                std::ptr::copy_nonoverlapping(length.as_ptr(), base.add(REPORT_OFFSET), 4);
                std::ptr::copy_nonoverlapping(
                    payload.as_ptr(),
                    base.add(REPORT_OFFSET + 4),
                    payload.len(),
                );
            }
        }
        failures
    }
}

#[cfg(windows)]
fn main() {
    std::process::exit(probe::run() as i32);
}

#[cfg(not(windows))]
fn main() {}
