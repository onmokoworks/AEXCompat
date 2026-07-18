# Crash Minidumps (2026-07-19)

Opt-in local crash dumps for issue #18. The default is off. A dump can contain
plug-in memory and is never copied into shareable reports.

## Enabling

Set the existing opt-in directory policy before launching the broker:

```powershell
$env:AEXCOMPAT_MINIDUMP_DIR = 'target/crash-dumps'
broker\target\release\aexcompat-harness.exe --render-experimental-smart `
    <plugin.aex> <input.png> <output.png>
```

The broker resolves the value below the repository `target/` tree, rejects
`.`/`..` traversal and reparse-point components, and creates one `CREATE_NEW`
file for the launch. It then authenticates the file's final path and inherits
the handle through the same explicit `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`
transport used by the trace channel. The worker receives only the numeric
handle value; it never receives a dump path and never re-opens one.

The broker policy bounds both dimensions of local storage:

- at most 16 retained dump files;
- at most 256 MiB cumulative dump bytes;
- at most 64 MiB for one dump.

A lock file serializes only the budget check and `CREATE_NEW` reservation. A
contender waits for the lock with a bounded timeout, then proceeds once the
reservation is complete. The authenticated dump handle remains owned by the
broker launch boundary until the worker exits. If the timeout expires, the
broker returns an explicit `TimedOut` launch error rather than silently
dropping the dispatch.

## Behavior

The worker starts a dedicated writer thread before dispatch. The SEH filter
copies the exception record and context into broker-owned storage, signals that
thread, and waits up to two seconds. `MiniDumpWriteDump` is never called from
the faulting thread. The writer loads `dbghelp.dll` from system32, uses the
authenticated inherited handle, and applies a 64 MiB write callback limit.

The worker emits one of these bounded diagnostics:

- `stage:minidump_written bytes=<n>`;
- `stage:minidump_failed reason=dbghelp_unavailable`;
- `stage:minidump_failed reason=entry_unavailable`;
- `stage:minidump_failed reason=handle_invalid`;
- `stage:minidump_failed reason=writer_unavailable`;
- `stage:minidump_failed reason=writer_timeout`;
- `stage:minidump_failed reason=write_failed code=<n>`.

Reports expose the configured directory as a managed display value and expose
only the bounded minidump marker. They do not include a full local path,
handle value, or dump contents.

## Verification

`--self-test-crash-minidump` raises a real access violation under the
production SEH guard. The native test supplies an already-created inheritable
file handle, then verifies a non-empty `MDMP` file and the byte bound.

`--self-test-crash-no-minidump` raises the same real access violation without
opt-in and verifies that no writer was attempted. Both modes are covered for
all three worker executables by `tests/test_worker_crash_minidump.py` when the
minihost binaries are built. Broker unit tests also cover cleanup of more than
the file-count cap worth of successful empty reservations, concurrent policy
lock queueing, and explicit lock timeout classification.
