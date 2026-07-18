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

The broker validates the directory before dispatch (fail-closed):

- must resolve under the repository `target/` tree, no `.`/`..` traversal;
- unlike world-dump directories it may be non-empty, because dumps accumulate
  across runs as create-new files.

## Behavior

- One dump per worker process, named `crash-<pid>.dmp`, written with
  `CREATE_NEW` so an existing dump is never overwritten.
- Written from the production `__except` filter (`capture_seh_exception`) that
  guards every effect entry call, and additionally from a
  `SetUnhandledExceptionFilter` top-level filter for crashes that never reach
  an `__except` (for example on a plug-in's own thread). The top-level filter
  returns `EXCEPTION_CONTINUE_SEARCH`, so the exit code and default handling
  are unchanged. Stack-overflow crashes may still be uncapturable because the
  handler itself needs stack; this limit is expected.
- `dbghelp.dll` is loaded dynamically (system32 only) and `MiniDumpWriteDump`
  resolved at crash time, so a machine without dbghelp degrades to a
  `stage:minidump_failed reason=dbghelp_unavailable` note rather than a
  secondary failure.
- The worker emits a `stage:minidump_written name=… bytes=…` or
  `stage:minidump_failed reason=…` line to stderr. The broker surfaces this as
  `minidump` in the worker diagnostics JSON (basename and reason only, never a
  full path) and records the managed dump directory as `minidump_directory` in
  the render report.

## Verification

`--self-test-crash-minidump <dir>` raises a real access violation under the
production guard and confirms a non-empty `MDMP`-signed dump landed. Covered by
`tests/test_worker_crash_minidump.py` for all three workers (registered in
`tests/local_artifact_tests.txt` since it runs a built worker).
