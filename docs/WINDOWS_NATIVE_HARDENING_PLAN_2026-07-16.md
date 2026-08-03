# Windows Native Hardening Plan (2026-07-16)

> Status note (2026-08-04): this is a hardening *plan* whose sections were
> appended over time, so earlier paragraphs (e.g. "is not connected to
> production dispatch") are superseded by later ones ("now
> production-connected for L2" under "Loader hardening"). The sealed load
> tree, restricted token, protected DACL, trusted-worker stage, and module
> audit ARE implemented and production-wired. The following are **not
> implemented anywhere in the codebase**: process mitigation policies,
> `JOB_OBJECT_UILIMIT_*` and other Job limits beyond kill-on-close plus
> process memory, low integrity levels, and AppContainer. The execution-mode
> table below is therefore aspirational beyond `compat` plus the shipped
> sealed/restricted launch. For the inventory of what actually exists, see
> `docs/ISOLATION_INVENTORY_2026-08-04.md` (issue #641).

## Current boundary

The native worker is a crash-containment boundary, not an untrusted-code security sandbox. It starts suspended, is assigned to a kill-on-close Job Object with a 512 MiB process-memory limit, inherits only explicit standard-output handles, and has bounded termination waits. It still runs with the broker user's token and can read or modify resources that user can access.

The broker and worker both authenticate the main AEX SHA-256, but the worker closes its hash input before `LoadLibraryExW` reopens the path. Windows does not provide a normal desktop API that loads a DLL from an already verified file handle. The path-based reopen leaves a replacement race, and `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR` permits adjacent dependency DLLs that are not currently part of the approved identity.

Consequently:

- Only trusted AEX builds may be approved in the current mode.
- A Job Object and timeout reduce crash and resource impact but do not prevent data access, registry writes, persistence, or network access.
- Low integrity would primarily prevent writes to medium-integrity objects; it would not provide confidentiality or network isolation.
- AppContainer provides a substantially stronger boundary but is expected to break GPU ICDs, COM, registry access, Adobe shared components, and legacy adjacent dependencies unless explicitly provisioned.

## Loader hardening

The first security implementation should be a broker-owned sealed private load tree because it addresses both the main-file replacement race and adjacent-DLL substitution with less compatibility risk than changing the token.

The reusable `sealed_load_tree` core is now implemented but is not connected to production dispatch. It accepts only caller-supplied main/dependency hashes and sizes, rejects basename/path/reparse/hardlink/case-collision violations, copies and reauthenticates into a random root, holds destination file handles without write/delete sharing, produces an order-independent canonical manifest digest, and performs non-recursive manifest-only cleanup.

Schema-v2 approval receipts now describe the main binary and up to 64 explicit adjacent dependencies as one strict load-tree identity. Windows ADS, control characters, trailing dot/space, DOS device names, unsafe basenames, malformed digests, and case-insensitive collisions fail closed. A restricted-token component creates and verifies a per-worker restricting SID. The ACL component applies a protected read/execute-only DACL through non-reparse handles held without delete sharing and constrains owner rights so implicit `WRITE_DAC` is not available to the worker. These components remain deliberately disconnected from production dispatch until one fail-closed launch transaction and adversarial worker-side access tests bind receipt validation, tree creation, ACL application, token creation, process launch, and cleanup together.

The launch transaction and `CreateProcessAsUserW` path are now production-connected for L2. The transaction independently authenticates and stages the trusted worker executable, owns both worker and plug-in trees until process exit, passes an absolute staged executable through `lpApplicationName`, fixes its current directory, and injects the authenticated plug-in path between typed worker arguments. A Windows adversarial worker proves that the protected trees permit required reads while denying existing-file writes, new children, deletes, and root/file `WRITE_DAC`; a common Restricted Code deny prevents a different restricted worker from modifying another stage. Hash mismatch, plug-in tamper, timeout, and cleanup E2E gates pass. ScatterMap and MaskOffset both complete their approved schema-v2 L2 runs through this path. There is no normal-token fallback.

This remains an integrity-oriented compatibility boundary rather than a confidentiality sandbox. Compatibility restricting SIDs and the current user SID allow the worker to traverse and read ordinary user/OS objects allowed by their existing ACLs. Another restricted worker may read a trusted worker stage, but a common Restricted Code deny prevents write, delete, DACL, or ownership mutation. L2, deterministic Classic Render, deterministic SmartFX, interactive image/audio rendering, UI events, sequence operations, options dialogs, and AEGP helper roundtrips now use the authenticated restricted transaction. `image_render.rs` contains no normal `run_isolated` call or fallback.

Native module audit v2 is now mandatory when the plugin path belongs to an AEXCompat sealed tree. Workers capture bounded `post_load`, every common EffectMain selector boundary, SEH exits, GPU begin/selector/setdown/end boundaries, and `pre_unload`, accumulating a sticky observed union so transiently loaded modules cannot disappear before final inspection. Only direct children of the sealed plugin root, the authenticated trusted-worker stage, and canonical System32 are permitted. Incomplete enumeration, unresolved paths, or unknown modules fail closed; reports expose only classified basenames. A current secure ScatterMap L2 observation records 11 phases with zero unknown modules in the union and both terminal snapshots. Direct legacy probe execution outside a sealed tree reports audit `not_required` to preserve test compatibility.

The broker now independently validates the audit on every successful production secure launch. It rejects truncated or malformed worker output, missing or unknown audit fields, non-passing terminal or cumulative snapshots, unsafe or duplicate basenames, fewer than three observations, nonzero unknown counts, and terminal entries absent from the cumulative union. Crash and timeout classifications remain reportable, while exit code zero without a complete audit fails closed. Test-only dummy workers must explicitly opt out; production L2, Classic, Smart, and image dispatch require the audit.

A strict Rust runtime-module policy core is connected to the authenticated GPU dispatch API. It validates at most 128 canonical absolute modules with backend, exact hash/size, expiry, optional signer/version identity, safe names, non-reparse/single-link files, collision rejection, and a worker-report classifier. GPU authorization also binds the classified report to a 32-byte session identity and revalidates it immediately before launch; missing, expired, wrong-backend, or wrong-session authorization fails closed. CPU dispatch and the fresh CPU fallback transaction do not consume GPU exceptions. Production policy generation remains pending because current CUDA/OpenCL/DirectX/OpenGL evidence lacks complete vendor UMD/ICD path, package, signature, adapter, driver, and OS identity.

The local runtime identity collector now records canonical path, SHA-256, size, PE machine, Windows volume serial, and file index while rejecting reparse points, hardlinks, malformed PE files, and non-regular files. Authenticode is deliberately reported as unsupported and the signature-required API fails closed until chain/catalog verification is implemented; no unsigned identity is promoted into runtime approval.

The Rust harness now supports session-only adjacent dependency manifests. Users explicitly add DLLs, review basename/hash/size, and can remove one or all entries. Any plugin or dependency change revokes session approval. Validation rejects more than 64 entries, unsafe Windows names, duplicate/case-colliding names, reparse points, hardlinks, changed hashes/sizes, and unknown JSON fields before dispatch; validated dependencies are included in both the initial render and GPU CPU-fallback sealed trees.

1. Open the approved AEX and explicitly allowed adjacent files while rejecting reparse points.
2. Hold source handles without write or delete sharing while recording SHA-256, size, volume serial, and file ID.
3. Copy only approved files into a random broker-owned staging directory and reauthenticate every copy.
4. Give the worker read/execute access only; deny file creation, rename, replacement, and deletion in the staged tree.
5. Hold staged file and directory handles for the entire worker lifetime.
6. Pass the staged AEX path plus a versioned manifest hash to the worker.
7. Restrict normal loader search to the staged AEX directory and System32.
8. After load, audit module paths and file identities; fail if a non-System32 module falls outside the staged manifest.
9. Remove the staged tree after the worker and all descendants terminate.

PE import parsing alone is insufficient because delay-load and runtime `LoadLibrary` calls are possible. The manifest must support explicit adjacent assets, and runtime module auditing remains required.

## Execution modes

| Mode | Intended behavior | Compatibility policy |
|---|---|---|
| `compat` | Preserve current execution while reporting effective token, mitigations, loader mode, and loaded modules. | Temporary baseline for trusted fixtures only. |
| `hardened-loader` | Connect the private load-tree core, add a restricted worker SID/DACL, require the authenticated dependency manifest, and audit loaded modules. | First target for broad default adoption; not yet production-connected. |
| `restricted` | Add `CreateRestrictedToken(DISABLE_MAX_PRIVILEGE)`, safe process mitigations, and bounded Job limits. | Candidate default for untrusted testing after fixture qualification. |
| `restricted-low` | Add low integrity and request-owned low-integrity transport/temp directories. | Opt in per fixture; protects medium-integrity objects from writes but not reads. |
| `strict` | Add capability-specific child-process, dynamic-code, Win32k, CFG, or signature restrictions. | Never enable globally without fixture capability evidence. |
| `appcontainer-experimental` | Capability-minimal AppContainer execution. | CPU-only dependency-free fixtures first; not a GPU or Adobe default. |

Safe mitigations should be introduced independently and measured. Blocking non-Microsoft binaries cannot be used because the AEX itself and Adobe, VC runtime, and GPU DLLs are non-Microsoft. Win32k disable, dynamic-code prohibition, strict CFG, and child-process prohibition can break UI, fonts, OpenGL/WGL, GPU JIT, or helper processes and therefore belong in capability-specific modes.

## Verification gates

Each mode must report its effective token, integrity level, mitigation flags, Job limits, staged manifest hash, and loaded-module audit. Promotion requires all applicable fixtures below to pass without silently falling back:

- Classic, SmartFX, deep-color, multi-input, and custom-UI workers.
- CUDA, OpenCL, DirectX, and OpenGL device paths plus CPU fallback.
- MaskOffset or another fixture with a reviewed adjacent dependency.
- Adobe SDK Gamma_Table, Supervisor, HistoGrid, and Grabba paths.
- Main-AEX replacement after authentication.
- Adjacent-DLL replace/add, hardlink substitution, junction/reparse substitution, delay-load, and dynamic-load attempts.
- System32 same-name dependency resolution.
- Low-integrity denial of medium-integrity file and registry writes.
- AppContainer denial of ungranted network access.

No mode may be labeled a security sandbox until its confidentiality, integrity, network, child-process, and persistence boundaries have direct adversarial evidence. Compatibility fallback must be explicit and user-visible rather than automatic.
