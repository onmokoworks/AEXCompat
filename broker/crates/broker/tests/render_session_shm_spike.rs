//! issue #98 項目5 spike: 常駐レンダリングセッションの共有メモリ設計の実証。
//!
//! 検証する主張:
//! 1. broker が作った無名 file mapping (pagefile 裏、SEC_COMMIT) は、名前も
//!    パスも介さず継承 handle だけで worker から MapViewOfFile でき、双方向に
//!    読み書きできる。
//! 2. 共有 view のページ touch は worker の private commit (PagefileUsage) に
//!    ほぼ課金されず、Job Object の ProcessMemoryLimit を食い潰さない。
//!    決め手として上限 (128MB) < section (256MB) の構成で全ページ touch が
//!    成功し、同時に上限超の private VirtualAlloc は失敗することを確認する。
//! 3. 継承した auto-reset event 対で broker↔worker の往復同期ができる
//!    (レイテンシは参考値として表示のみ、assert しない)。
//!
//! launch は windows_process.rs の run_isolated_impl と同じ形状
//! (CreateProcessW + PROC_THREAD_ATTRIBUTE_HANDLE_LIST + Job assign 後
//! resume) をテスト内に再現する。spike なので production API は変更しない。
//! restricted token は #731 で撤去済みなので、この spike も通常トークンで
//! 起動する (証明対象は継承 handle と Job Object の課金であってトークンでは
//! ない)。

#[cfg(windows)]
mod windows_e2e {
    use std::ffi::c_void;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::ptr::{null, null_mut};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{
        CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
        TerminateJobObject,
    };
    use windows_sys::Win32::System::Memory::{
        CreateFileMappingW, FILE_MAP_ALL_ACCESS, MapViewOfFile, PAGE_READWRITE, UnmapViewOfFile,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateEventW,
        CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
        GetExitCodeProcess, InitializeProcThreadAttributeList, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
        PROCESS_INFORMATION, ResumeThread, STARTUPINFOEXW, SetEvent, TerminateProcess,
        UpdateProcThreadAttribute, WaitForSingleObject,
    };

    const SECTION_BYTES: usize = 256 * 1024 * 1024;
    const JOB_PROCESS_MEMORY_LIMIT: usize = 128 * 1024 * 1024;
    const PRIVATE_PROBE_BYTES: usize = 192 * 1024 * 1024;
    const PINGPONG_ROUNDS: usize = 2000;
    // 共有 view の touch が private commit に「ほぼ」課金されない判定のしきい値。
    // 課金される場合は section 全量 (256MB) 級の増分になるので桁で区別できる。
    const PRIVATE_COMMIT_DELTA_TOLERANCE: u64 = 32 * 1024 * 1024;

    const ACK_OFFSET: usize = 128;
    const ACK: &[u8] = b"AEXCOMPAT_SPIKE_ACK1";
    const REPORT_OFFSET: usize = 4096;
    const PATTERN_BYTES: usize = 64;

    struct OwnedHandle(HANDLE);
    impl OwnedHandle {
        fn new(value: HANDLE) -> io::Result<Self> {
            if value.is_null() || value == INVALID_HANDLE_VALUE {
                Err(io::Error::last_os_error())
            } else {
                Ok(Self(value))
            }
        }
        fn raw(&self) -> HANDLE {
            self.0
        }
    }
    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    struct TempTree(PathBuf);
    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct View(*mut u8);
    impl Drop for View {
        fn drop(&mut self) {
            if !self.0.is_null() {
                let address = windows_sys::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: self.0 as *mut c_void,
                };
                unsafe {
                    UnmapViewOfFile(address);
                }
            }
        }
    }

    fn head_pattern(index: usize) -> u8 {
        (index as u8).wrapping_mul(31).wrapping_add(7)
    }

    fn tail_pattern(index: usize) -> u8 {
        (index as u8).wrapping_mul(59).wrapping_add(3)
    }

    fn inheritable() -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        }
    }

    fn environment_block(extra: &[(&str, String)]) -> Vec<u16> {
        let mut entries: Vec<(String, std::ffi::OsString, std::ffi::OsString)> =
            std::env::vars_os()
                .map(|(key, value)| {
                    let normalized = key.to_string_lossy().to_ascii_uppercase();
                    (normalized, key, value)
                })
                .collect();
        for (key, value) in extra {
            entries.push(((*key).into(), (*key).into(), value.clone().into()));
        }
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        let mut block = Vec::new();
        for (_, key, value) in entries {
            block.extend(key.encode_wide());
            block.push('=' as u16);
            block.extend(value.encode_wide());
            block.push(0);
        }
        block.push(0);
        block
    }

    fn build_fixture() -> PathBuf {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let status = std::process::Command::new(env!("CARGO"))
            .args(["build", "--manifest-path"])
            .arg(manifest)
            .args(["-p", "dummy-workers", "--bin", "session_shm_probe"])
            .status()
            .expect("run cargo build for session shm probe fixture");
        assert!(status.success(), "session shm probe fixture build failed");
        let deps = std::env::current_exe().unwrap();
        deps.parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("session_shm_probe.exe")
    }

    fn parent_private_commit() -> Option<u64> {
        use windows_sys::Win32::System::ProcessStatus::{
            K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut info: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            K32GetProcessMemoryInfo(
                GetCurrentProcess(),
                &mut info,
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            )
        };
        (ok != 0).then_some(info.PagefileUsage as u64)
    }

    #[test]
    fn worker_maps_inherited_anonymous_section_without_commit_charge() {
        let fixture = build_fixture();
        let root = TempTree(std::env::temp_dir().join(format!(
            "aexcompat-session-shm-spike-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        )));
        std::fs::create_dir(&root.0).unwrap();
        std::fs::copy(fixture, root.0.join("session_shm_probe.exe")).unwrap();

        let parent_commit_before_section = parent_private_commit();

        // 1. 無名 file mapping (pagefile 裏、SEC_COMMIT 既定) を継承可で作成。
        let mut security = inheritable();
        let section = OwnedHandle::new(unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                &mut security,
                PAGE_READWRITE,
                (SECTION_BYTES as u64 >> 32) as u32,
                SECTION_BYTES as u32,
                null(),
            )
        })
        .expect("create anonymous section");
        let view_address = unsafe { MapViewOfFile(section.raw(), FILE_MAP_ALL_ACCESS, 0, 0, 0) };
        assert!(!view_address.Value.is_null(), "parent MapViewOfFile failed");
        let view = View(view_address.Value as *mut u8);
        unsafe {
            for index in 0..PATTERN_BYTES {
                view.0.add(index).write_volatile(head_pattern(index));
            }
            for index in 0..PATTERN_BYTES {
                view.0
                    .add(SECTION_BYTES - PATTERN_BYTES + index)
                    .write_volatile(tail_pattern(index));
            }
        }
        let parent_commit_after_pattern = parent_private_commit();

        // 2. 継承可 auto-reset event 対 (req: broker→worker, rsp: worker→broker)。
        let mut security = inheritable();
        let req = OwnedHandle::new(unsafe { CreateEventW(&mut security, 0, 0, null()) }).unwrap();
        let mut security = inheritable();
        let rsp = OwnedHandle::new(unsafe { CreateEventW(&mut security, 0, 0, null()) }).unwrap();

        // 3. Job Object: production と同じ limit flags、ただし上限は section より
        //    小さい 128MB。共有 view が課金されるなら worker は生き残れない。
        let job = OwnedHandle::new(unsafe { CreateJobObjectW(null(), null()) }).unwrap();
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
        limits.ProcessMemoryLimit = JOB_PROCESS_MEMORY_LIMIT;
        assert_ne!(
            unsafe {
                SetInformationJobObject(
                    job.raw(),
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            },
            0,
            "SetInformationJobObject failed"
        );

        // 4. production launch 形状: handle list で継承を絞り、環境変数で handle
        //    番号を渡し、Job assign 後に resume。
        let mut attribute_size = 0usize;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut attribute_size);
        }
        let words = attribute_size.div_ceil(std::mem::size_of::<usize>());
        let mut attribute_storage = vec![0usize; words];
        let attribute_list = attribute_storage.as_mut_ptr().cast();
        assert_ne!(
            unsafe { InitializeProcThreadAttributeList(attribute_list, 1, 0, &mut attribute_size) },
            0,
            "InitializeProcThreadAttributeList failed"
        );
        struct AttributeGuard(*mut c_void);
        impl Drop for AttributeGuard {
            fn drop(&mut self) {
                unsafe {
                    DeleteProcThreadAttributeList(self.0);
                }
            }
        }
        let _attribute_guard = AttributeGuard(attribute_list);
        let mut inherited = vec![section.raw(), req.raw(), rsp.raw()];
        assert_ne!(
            unsafe {
                UpdateProcThreadAttribute(
                    attribute_list,
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                    inherited.as_mut_ptr().cast(),
                    std::mem::size_of_val(inherited.as_slice()),
                    null_mut(),
                    null_mut(),
                )
            },
            0,
            "UpdateProcThreadAttribute failed"
        );

        let executable = root.0.join("session_shm_probe.exe");
        let application: Vec<u16> = executable.as_os_str().encode_wide().chain([0]).collect();
        let mut command: Vec<u16> = format!("\"{}\"", executable.display())
            .encode_utf16()
            .chain([0])
            .collect();
        let current_directory: Vec<u16> = root.0.as_os_str().encode_wide().chain([0]).collect();
        let mut environment = environment_block(&[
            (
                "AEXCOMPAT_SPIKE_SECTION_HANDLE",
                (section.raw() as usize).to_string(),
            ),
            (
                "AEXCOMPAT_SPIKE_REQ_EVENT",
                (req.raw() as usize).to_string(),
            ),
            (
                "AEXCOMPAT_SPIKE_RSP_EVENT",
                (rsp.raw() as usize).to_string(),
            ),
            ("AEXCOMPAT_SPIKE_SECTION_BYTES", SECTION_BYTES.to_string()),
            (
                "AEXCOMPAT_SPIKE_PINGPONG_ROUNDS",
                PINGPONG_ROUNDS.to_string(),
            ),
            (
                "AEXCOMPAT_SPIKE_PRIVATE_PROBE_BYTES",
                PRIVATE_PROBE_BYTES.to_string(),
            ),
        ]);

        let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.lpAttributeList = attribute_list;
        let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        let created = unsafe {
            CreateProcessW(
                application.as_ptr(),
                command.as_mut_ptr(),
                null(),
                null(),
                1,
                EXTENDED_STARTUPINFO_PRESENT
                    | CREATE_SUSPENDED
                    | CREATE_NO_WINDOW
                    | CREATE_UNICODE_ENVIRONMENT,
                environment.as_mut_ptr().cast(),
                current_directory.as_ptr(),
                &startup.StartupInfo,
                &mut process,
            )
        };
        assert_ne!(
            created,
            0,
            "CreateProcessW failed: {}",
            io::Error::last_os_error()
        );
        let process_handle = OwnedHandle::new(process.hProcess).unwrap();
        let thread_handle = OwnedHandle::new(process.hThread).unwrap();

        // production の SuspendedProcessCleanup と同型: 作成後〜resume 成功
        // までに panic した場合、suspended な子を残さない。Job 割り当て前は
        // TerminateProcess、割り当て後は TerminateJobObject で始末する。
        struct SuspendedProcessCleanup {
            process: HANDLE,
            job: HANDLE,
            assigned_to_job: bool,
            armed: bool,
        }
        impl Drop for SuspendedProcessCleanup {
            fn drop(&mut self) {
                if !self.armed {
                    return;
                }
                unsafe {
                    if self.assigned_to_job {
                        TerminateJobObject(self.job, 0xDEAD);
                    } else {
                        TerminateProcess(self.process, 0xDEAD);
                    }
                    WaitForSingleObject(self.process, 5_000);
                }
            }
        }
        let mut suspended_cleanup = SuspendedProcessCleanup {
            process: process_handle.raw(),
            job: job.raw(),
            assigned_to_job: false,
            armed: true,
        };
        assert_ne!(
            unsafe { AssignProcessToJobObject(job.raw(), process_handle.raw()) },
            0,
            "AssignProcessToJobObject failed"
        );
        suspended_cleanup.assigned_to_job = true;
        assert_ne!(
            unsafe { ResumeThread(thread_handle.raw()) },
            u32::MAX,
            "ResumeThread failed"
        );
        suspended_cleanup.armed = false;
        drop(thread_handle);

        // 5. worker の ready (map + パターン検証 + ACK 書き込み完了) を待つ。
        //    初回はプロセス起動 + AV スキャン込みなので長めに取る。
        let ready = unsafe { WaitForSingleObject(rsp.raw(), 60_000) };
        assert_eq!(ready, WAIT_OBJECT_0, "worker never signaled ready");

        // 6. イベント往復レイテンシ (参考値)。auto-reset 対で ping-pong。
        let mut round_trips: Vec<Duration> = Vec::with_capacity(PINGPONG_ROUNDS);
        for _ in 0..PINGPONG_ROUNDS {
            let start = Instant::now();
            assert_ne!(unsafe { SetEvent(req.raw()) }, 0, "SetEvent(req) failed");
            let wait = unsafe { WaitForSingleObject(rsp.raw(), 10_000) };
            assert_eq!(wait, WAIT_OBJECT_0, "ping-pong response timed out");
            round_trips.push(start.elapsed());
        }
        round_trips.sort();
        let median = round_trips[round_trips.len() / 2];
        let minimum = round_trips[0];
        let maximum = round_trips[round_trips.len() - 1];

        // 7. worker の終了 (全ページ touch + private probe + レポート書き込み)。
        let exited = unsafe { WaitForSingleObject(process_handle.raw(), 120_000) };
        assert_eq!(exited, WAIT_OBJECT_0, "worker did not exit");
        let mut exit_code = 0u32;
        assert_ne!(
            unsafe { GetExitCodeProcess(process_handle.raw(), &mut exit_code) },
            0
        );
        assert_eq!(
            exit_code & 0xFFFF,
            0,
            "probe failure bitmap {:#09b} (map_error in high bits: {}): \
             env, map, pattern, counters, pingpong, cap-not-enforced, report",
            exit_code & 0xFFFF,
            exit_code >> 16,
        );

        // 8. 共有メモリ経由の観測を回収。ACK は worker 書き込みが親から見える
        //    証拠、レポートは worker 自身のメモリカウンタ。
        let mut ack = [0u8; 20];
        unsafe {
            std::ptr::copy_nonoverlapping(view.0.add(ACK_OFFSET), ack.as_mut_ptr(), ACK.len());
        }
        assert_eq!(&ack[..], ACK, "worker ack marker missing from shared view");
        let mut length_bytes = [0u8; 4];
        unsafe {
            std::ptr::copy_nonoverlapping(view.0.add(REPORT_OFFSET), length_bytes.as_mut_ptr(), 4);
        }
        let length = u32::from_le_bytes(length_bytes) as usize;
        assert!(length > 0 && length < 4096, "report length out of range");
        let mut payload = vec![0u8; length];
        unsafe {
            std::ptr::copy_nonoverlapping(
                view.0.add(REPORT_OFFSET + 4),
                payload.as_mut_ptr(),
                length,
            );
        }
        let report: serde_json::Value =
            serde_json::from_slice(&payload).expect("parse worker report JSON");

        let pagefile_before_map = report["pagefile_before_map"].as_u64().unwrap();
        let pagefile_after_touch = report["pagefile_after_touch"].as_u64().unwrap();
        let peak_pagefile = report["peak_pagefile_after_touch"].as_u64().unwrap();
        let working_set = report["working_set_after_touch"].as_u64().unwrap();
        let private_alloc_failed = report["private_alloc_failed"].as_bool().unwrap();

        // 9. Job 側の集計 (プロセス終了後も job handle が開いている間は残る)。
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        let queried = unsafe {
            QueryInformationJobObject(
                job.raw(),
                JobObjectExtendedLimitInformation,
                &mut info as *mut _ as *mut c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                null_mut(),
            )
        };
        let peak_process_memory = (queried != 0).then_some(info.PeakProcessMemoryUsed as u64);
        unsafe {
            TerminateJobObject(job.raw(), 0);
        }

        println!("=== issue #98 item 5 spike observations ===");
        println!(
            "section {} MiB, job ProcessMemoryLimit {} MiB, private probe {} MiB",
            SECTION_BYTES / (1024 * 1024),
            JOB_PROCESS_MEMORY_LIMIT / (1024 * 1024),
            PRIVATE_PROBE_BYTES / (1024 * 1024),
        );
        println!(
            "parent private commit: before section {:?} -> after map+pattern {:?}",
            parent_commit_before_section, parent_commit_after_pattern
        );
        println!(
            "worker private commit (PagefileUsage): before map {} KiB -> after touch {} KiB (peak {} KiB)",
            pagefile_before_map / 1024,
            pagefile_after_touch / 1024,
            peak_pagefile / 1024,
        );
        println!("worker working set after touch: {} KiB", working_set / 1024);
        println!(
            "job PeakProcessMemoryUsed: {:?} KiB",
            peak_process_memory.map(|v| v / 1024)
        );
        println!(
            "event ping-pong round trip over {} rounds: median {:?}, min {:?}, max {:?}",
            PINGPONG_ROUNDS, median, minimum, maximum
        );

        // 主張2の判定: 共有 view 全 touch 後も private commit の増分は section
        // 全量 (256MB) より桁で小さい。かつ上限は同プロセスで実効 (private
        // alloc 失敗、probe の bitmap でも検証済みだが冗長に確認する)。
        let delta = pagefile_after_touch.saturating_sub(pagefile_before_map);
        assert!(
            delta < PRIVATE_COMMIT_DELTA_TOLERANCE,
            "shared view touch charged {} KiB to worker private commit",
            delta / 1024
        );
        assert!(private_alloc_failed, "job commit cap was not enforced");
        if let Some(peak) = peak_process_memory {
            assert!(
                peak < JOB_PROCESS_MEMORY_LIMIT as u64,
                "job peak process memory {} KiB reached the cap",
                peak / 1024
            );
        }
    }
}
