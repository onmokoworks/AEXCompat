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
file for the launch. It then authenticates the file's final path and keeps the
file handle broker-private. The worker receives only an explicit pipe transport
through the same `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` mechanism used by the
trace channel; a broker reader copies at most 64 MiB into the authenticated
file. The worker never receives a dump path or the dump file handle and never
re-opens one.

The managed dump directory and each reservation file use a protected DACL
granting full access only to LocalSystem and the object owner, because a dump
can contain arbitrary process memory. Publication renames the still-open authenticated handle from
`.dmp.part` to `.dmp`; no close-then-path-rename window exists.

The broker policy bounds both dimensions of local storage:

- at most 16 retained dump files;
- at most 256 MiB cumulative dump bytes;
- at most 64 MiB for one dump, enforced by the broker reader;
- each in-flight `.dmp.part` reservation accounts for the full 64 MiB maximum.

Under the policy lock, exclusively openable stale `.dmp.part` files are
authenticated and deleted by handle before budgeting. Active reservations are
held with share mode zero and remain charged at the full per-dump maximum.

A lock file serializes only the budget check and `CREATE_NEW` reservation. A
contender waits for the lock with a bounded timeout, then proceeds once the
reservation is complete. The authenticated dump handle remains owned by the
broker launch boundary until the worker exits. If the timeout expires, the
broker returns an explicit `TimedOut` launch error rather than silently
dropping the dispatch.

## Behavior

The worker preloads system32 `dbghelp.dll` and resolves `MiniDumpWriteDump`
before any plug-in load or execution. It then starts a dedicated writer thread
before dispatch. The SEH filter
copies the exception record and context into broker-owned storage, signals that
thread, and waits up to two seconds. `MiniDumpWriteDump` is never called from
the faulting thread. The writer uses the inherited pipe and waits for the
broker's bounded-copy acknowledgement.
The broker reader stops immediately on overflow and uses a cancellable polling
loop, so a worker that duplicates or retains its pipe writer cannot block
broker teardown indefinitely.

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
production SEH guard. The native test supplies the same inheritable dump pipe
and broker acknowledgement pipe used in production, copies the stream into a
file, then verifies a non-empty `MDMP` file and the byte bound.

`--self-test-crash-no-minidump` raises the same real access violation without
opt-in and verifies that no writer was attempted. Both modes are covered for
all three worker executables by `tests/test_worker_crash_minidump.py` when the
minihost binaries are built. Broker unit tests also cover cleanup of more than
the file-count cap worth of successful empty reservations, concurrent policy
lock queueing, and explicit lock timeout classification.
