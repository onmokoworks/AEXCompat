# Crash Minidumps (2026-07-19)

Opt-in local crash dumps (issue #18). When a worker crashes inside the
SEH-guarded effect call, the existing diagnostics record the exception code,
faulting address, module, and selector but no stack. With a minidump directory
configured, the worker also writes a create-new `.dmp` so the crashing call
stack in the plug-in can be reconstructed in a debugger. Default off; local
only; the dump contains plug-in memory and is never serialized into shareable
reports.

## Enabling

```powershell
$env:AEXCOMPAT_MINIDUMP_DIR = 'target/crash-dumps'
broker\target\release\aexcompat-harness.exe --render-experimental-smart `
    <plugin.aex> <input.png> <output.png>
```

The flag is injected once in `secure_launch`, the single choke point every
sealed worker dispatch funnels through, so it applies uniformly to every
launch path: the experimental image render/smart routes, the schema-v2
production L2/render/SmartFX dispatches (`l2.rs`, `render.rs`, `smart.rs`,
`render_request.rs`), and future callers, not just the image-render wrapper.
The worker consumes the trailing `--minidump-v1 <dir>` pair before its
argc-exact mode dispatch, so it is transparent to the per-kind argument
parsing.

The broker validates the directory before dispatch (fail-closed):

- must resolve under the repository `target/` tree, no `.`/`..` traversal;
- unlike world-dump directories it may be non-empty, because dumps accumulate
  across runs as create-new files.

## Behavior

Enabling happens once, before the plug-in loads, in `minidump::enable()`:

- **The directory is pinned by an open handle** (`FILE_FLAG_BACKUP_SEMANTICS |
  FILE_FLAG_OPEN_REPARSE_POINT`) and verified to be a real directory, not a
  reparse point. Holding the handle blocks the directory from being renamed or
  replaced, and its canonical path is resolved once via
  `GetFinalPathNameByHandle`. The dump is later created under that pinned path
  with `FILE_FLAG_OPEN_REPARSE_POINT`, so a plug-in (running under the same
  user token) cannot redirect the dump outside the managed tree by swapping the
  directory or leaf for a junction after the broker validated it.
- **`dbghelp.dll` is loaded and `MiniDumpWriteDump` resolved up front**, and a
  dedicated dumper thread is pre-started. The crash path does no loader work.
- **Directory accumulation cap** (`kMaxDirFiles` = 64, `kMaxDirBytes` = 512 MiB):
  if the directory already holds that many `crash-*.dmp` files or bytes,
  enabling degrades (keeps rendering, no dumps) rather than letting repeated
  crashes exhaust the target tree.
- Missing dbghelp, event/thread creation failure, or the dir cap all degrade
  (render continues without dumps); only a bad directory fails the worker
  closed. Enabling is opt-in, so nothing above runs by default.

On a crash:

- The SEH filter (`capture_seh_exception`, guarding every effect entry) and the
  `SetUnhandledExceptionFilter` top-level filter (for crashes that never reach
  an `__except`, e.g. on a plug-in's own thread) both route to
  `write_crash_minidump`. **`MiniDumpWriteDump` never runs on the faulting
  thread**: the filter copies the exception record and context to stable
  storage, signals the pre-started dumper thread, and waits with a 15 s
  timeout. A loader-deadlocked dump therefore cannot hang crash containment —
  on timeout the filter returns and the worker still converts to 512 and exits.
  The top-level filter returns `EXCEPTION_CONTINUE_SEARCH`, so the exit code is
  unchanged. Stack-overflow crashes may still be uncapturable (the handler
  itself needs stack); this limit is expected.
- One dump per process, `crash-<pid>.dmp`, `CREATE_NEW` (never overwrites).
  A **per-file cap** (`kMaxFileBytes` = 64 MiB) deletes and reports
  `size_exceeded` if a dump exceeds it.
- The worker emits `stage:minidump_written name=… bytes=…` or
  `stage:minidump_failed reason=…` to stderr (reasons include `timeout`,
  `size_exceeded`, `dir_cap`, `create_failed`, `write_failed`,
  `dbghelp_unavailable`). The broker validates this against the exact
  worker-owned shape and surfaces it as `minidump` in the diagnostics JSON
  (reason/bytes only, never a path), plus `minidump_directory` in the report.

## Verification

`tests/test_worker_crash_minidump.py` (all three workers, registered in
`tests/local_artifact_tests.txt` since it runs built workers):

- `--self-test-crash-minidump <dir>` raises a real access violation under the
  production guard and confirms a non-empty `MDMP`-signed dump landed (proving
  the dedicated-thread path end to end).
- `--self-test-crash-no-minidump <dir>` raises the same guarded crash with
  minidumps disabled and confirms no dump is produced (the default-off path).
- A directory pre-filled past the cap confirms enabling degrades and no further
  dump is written.
