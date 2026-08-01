# macOS Apple Silicon worker security boundary (Issue #637)

## Release decision and security tiers

This hardening is a practical pre-publication containment boundary, not a claim
that arbitrary AEX code is safe. The two backends deliberately have different
tiers:

| Tier | Execution | Admission | Guarantee |
| --- | --- | --- | --- |
| `apple_silicon_unicorn_guest` | arm64 process, x64 PE in Unicorn guest memory | default correctness backend | guest-memory plus process/resource containment; not a complete hostile-code sandbox |
| `apple_silicon_native_carrier_trusted_only` | x86_64 process under Rosetta, PE code runs natively | default off; requires both `AEXCOMPAT_NATIVE_CARRIER=1` and `AEXCOMPAT_NATIVE_CARRIER_TRUSTED=1` | crash/resource containment only; no confidentiality guarantee |

Native use, the trusted-only decision, the selected tier, and every Unicorn
fallback reason are recorded in the macOS report extension. They are not added
to the common report schema by this change.

## Implemented process boundary

Both setup and resident workers go through
`broker/crates/harness/src/macos_worker_controller.rs`.

* A broker-owned, random, mode `0700` temporary directory is the worker CWD.
  Only worker, AEX, and required input/output slots are copied there. Each copy
  is SHA-256 checked against its source and the tree is audited before removal.
* The environment is cleared and rebuilt with only `PATH`, `LANG`, `LC_ALL`,
  `TMPDIR`, and `AEXCOMPAT_MACOS_SECURITY_TIER`. `HOME`, shell configuration,
  credential-agent variables, and `DYLD_*` are not inherited.
* stdin is `/dev/null`; stdout and stderr are dedicated pipes. Rust's
  close-on-exec behavior leaves no application FD allowlist beyond the standard
  streams and the resident protocol pipes created for that session.
* Every worker is a process-group leader. Timeout, disconnect, and owner drop
  send group `SIGTERM`, allow 150 ms, send group `SIGKILL`, reap the leader, and
  probe the process group for residual members. Descendants observed through
  public `proc_listchildpids` are also signaled individually, including a child
  that created a different process group/session; UUID and process-start-time
  checks prevent a recycled PID from being targeted. A residual is
  `macos_worker_residual_process`, never a reusable session.
* Cleanup is explicit on success and failure, with a `Drop` fallback. Cleanup
  errors are preserved as `macos_worker_cleanup` instead of being rounded to
  success.

This boundary does not prevent a native worker from reading files already
permitted to the user or opening network connections. A process group is a
lifecycle boundary, not a confidentiality sandbox.

## Resource policy

| Resource | Enforcement |
| --- | --- |
| CPU | `RLIMIT_CPU=90s` plus setup/render/close wall deadlines |
| resident memory/physical footprint | broker polling through public `proc_pid_rusage`, 1 GiB |
| address space | not lowered: `RLIMIT_RSS` is not enforceable on tested macOS and a useful `RLIMIT_AS` cannot be safely imposed after framework mappings |
| open files | `RLIMIT_NOFILE=32` |
| output file | `RLIMIT_FSIZE=256 MiB` |
| stdout/stderr | broker readers, 256 KiB / 128 KiB |
| resident protocol | 64 KiB per frame in the worker and 1 MiB cumulative in the broker |
| staged artifacts/temp directory | at most 4 files and 512 MiB total; setup narrows this to 2 files |
| child processes | public `proc_listchildpids`, limit 0; any child invalidates and terminates the group |
| deadlines | native setup 2s; resident startup 10s; render 30s; close 2s |

macOS-only failure prefixes include `macos_worker_launch`,
`macos_worker_timeout`, `macos_worker_memory_limit`,
`macos_worker_child_limit`, `macos_worker_output_limit`,
`macos_worker_protocol_limit`, `macos_worker_artifact_limit`,
`macos_worker_cleanup`, and `macos_worker_residual_process`.

## Guest and native execution boundaries

The shared PE parser rejects writable-and-executable sections. Unicorn maps the
image writable for relocation/import installation, then seals image pages as
read-only, read/write, or read/execute and seals generated stubs read/execute.
Any page union that would be W+X fails. Existing guest range, pointer, integer,
rowbytes, dimensions, pixel size, arena, callback, unknown-instruction, and
per-dispatch timeout checks remain in force. Unknown scalar imports no longer
receive a generic zero-success stub; only explicitly implemented imports are
admitted and all others trap. Resident frame buffers are rebuilt/reset for each
render by the Classic lifecycle.

The native carrier remains opt-in and trusted-only, leaves `DllMain` disabled by
default, uses a bounded 256 MiB host arena, rejects imports without typed native
implementations, and applies PE section protections before guest calls. It
snapshots public dyld image names at engine creation and rejects a call if a new
Mach-O image appears. This is detection, not prevention, and a malicious native
guest may act before detection; the controller discards that process/session.

No plugin-specific identity, RVA, effect algorithm, or compatibility hack was
added.

## Signing, Hardened Runtime, App Sandbox, and distribution

Apple requires Hardened Runtime for notarized Developer ID software and advises
granting only required runtime exceptions. The arm64 Unicorn helper currently
needs `com.apple.security.cs.allow-jit` and
`com.apple.security.cs.allow-unsigned-executable-memory`; the Rosetta native
helper needs only unsigned executable memory for mapped PE code. Library
validation remains enabled, and `get-task-allow`, DYLD environment variables,
disable-library-validation, and disable-executable-page-protection are rejected
by `tools/verify-macos-aex-carriers.sh`.

`tools/sign-macos-aex-carriers.sh` signs both thin helpers with Hardened Runtime.
It defaults to an ad-hoc identity for local tests; setting
`AEXCOMPAT_CODESIGN_IDENTITY` uses a Developer ID identity and a secure
timestamp. `tools/verify-macos-aex-carriers.sh` verifies signatures,
architectures, runtime flags, entitlements, and both launch paths. The x86_64
helper is intentionally a separate thin executable so Apple Silicon launches it
through Rosetta rather than accidentally selecting an arm64 slice.

App Sandbox is useful for limiting file/network reach, but it is not enabled in
this minimum release boundary. Arbitrary user-selected AEX and staged helper
execution require a bundle/container and security-scoped access design that the
current command-line packaging does not have. Apple also requires an embedded
tool in a sandboxed app to inherit the containing app's sandbox. Adding a broad
set of exceptions merely to preserve compatibility would undermine the claim.
App Sandbox therefore remains a post-release prototype and must not be described
as making arbitrary AEX safe.

Developer ID signing, `notarytool submit --wait`, inspection of the complete
notary log, stapling the ticket to a bundle/DMG/package, `codesign --strict`, and
Gatekeeper (`spctl`) assessment must be run on the final nested-code-signed
distribution. Standalone binaries can be notarized as archive contents but
cannot themselves carry a stapled ticket, so the release must define a bundle,
DMG, or package. This work did not submit, staple, publish, or push a release.

Apple references:

* [Hardened Runtime](https://developer.apple.com/documentation/security/hardened-runtime)
* [Porting JIT compilers to Apple silicon](https://developer.apple.com/documentation/Apple-Silicon/porting-just-in-time-compilers-to-apple-silicon)
* [Notarizing macOS software before distribution](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)
* [Customizing the notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow)
* [App Sandbox](https://developer.apple.com/documentation/security/app-sandbox)
* [Protecting user data with App Sandbox](https://developer.apple.com/documentation/security/protecting-user-data-with-app-sandbox)
* [Building a universal macOS binary](https://developer.apple.com/documentation/Apple-Silicon/building-a-universal-macos-binary)

## Controlled-test status

Self-authored/generated fixtures cover malformed PE, writable-executable PE,
invalid entry/range checks, unsupported import, Unicorn infinite-loop timeout,
arena/allocation bounds, stdout flood, child spawn and group cleanup, artifact
tree overflow, broker disconnect/drop, worker timeout/crash, residual detection,
cleanup failure classification, bounded protocol frames and cumulative reports,
repeated resident lifecycle, native failure to Unicorn fallback, and stable dyld
snapshots. The child fixture also creates a new session to prove that observed
descendants are killed independently of process-group membership. Release builds
exercise staged arm64 and Rosetta workers from private session directories.

The private frozen corpus is not stored in this repository and its five-entry
locator was absent in this worktree. Spotlight did locate SHA-matching frozen
copies of `olm-blur` and `olm-colorkeep` plus a pre-existing campaign input.
Both rendered successfully twice through the arm64 Unicorn worker and each pair
was byte-identical (`OLMBlur` PNG SHA-256
`3c414b56060f87664018d89bbce18c681d6f7662b9b991d353812c4eadd22f6d`;
`ColorKeep` PNG SHA-256
`b1a5bbb1bc106c433d1d239b074427c7685edc4af4d3a06234bc30ac0e683242`).
This caught and corrected an over-broad unknown-import rejection by replacing
generic zero behavior with a finite, library-qualified set of typed or
deterministic Windows runtime callbacks. The remaining three frozen identities
and the canonical matrix still require the corpus custodian's full replay before
publication; the two-case smoke test is not a substitute for that gate.

The x86_64 Release test binary also exposed a host/Rosetta issue. A parallel run
left several concurrent Unicorn initialization/OpenCL threads uninterruptible;
a later `--test-threads=1` run isolated a reproducible stop in
`win64_import_bridge_executes_real_apple_gpu_kernel_and_cleans_up`. Each process
remained in `UE` state after SIGKILL. The 18 native-carrier-specific tests and
the staged Rosetta launch pass; the blocked test is the Unicorn real-Apple-GPU
bridge compiled for x86_64, not a native-carrier callback. Publication must not
call the complete x86_64 suite green until the host is rebooted, the residuals
are gone, and that Rosetta/OpenCL combination is either made interruptible or
excluded with an explicit architecture rationale. This is exactly the residual
failure class the production controller reports and refuses to reuse.

## Diagnostics and data handling

Crash reports, stderr, failure reports, staged path names, and notary logs may
contain plugin names, user paths, or environment-derived diagnostics. Keep them
local/private by default, apply existing redaction before sharing, bound their
size, and never publish a crash report or notary log automatically. Session
artifacts are deleted after the structured result is captured.

## Remaining limits and roadmap

Pre-publication blockers are: a credentialed Developer ID + notarization run on
the final package; the remaining three frozen identities plus canonical
success-set/byte-exact replay; and a reboot followed by a zero-residual decision
for the x86_64/Rosetta Unicorn real-OpenCL test.
The existing controller, local Hardened Runtime signing test, distinct tiers,
and documented non-guarantees are otherwise a sufficient minimum boundary.

Post-release work is a narrowly entitled App Sandbox helper prototype,
`MAP_JIT`/write-protect integration that removes the arm64 unsigned-executable-
memory exception, stronger native library admission before execution, and a
packaged crash-diagnostic consent/redaction workflow.
