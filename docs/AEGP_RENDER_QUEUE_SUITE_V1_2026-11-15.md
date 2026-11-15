# AEGP Render Queue Suite v1 — #1837 (2026-11-15)

## Design

`AEGP Render Queue Suite` acquisition version 1 follows the external SDK
`AE_GeneralPlug.h`: slot 0 AddCompToRenderQueue, slot 1 SetRenderQueueState,
slot 2 GetRenderQueueState. State is an unsigned 32-bit SDK value; STOPPED is 0.
The SDK remains external and is not redistributed.

State queries return STOPPED; a null destination returns error 4. The worker
request parser, render-environment payload (quality, field, shutter and output
rectangle), and broker request models contain no final/render-queue intent.
Classic/SmartFX, float depth and a resident session do not imply an AE render
queue. There is no simulated queue or transition to RENDERING.

Both mutators always return error 4, leave the state unchanged, and use the
existing unsupported-suite diagnostic record (suite name/version/slot/count).
Even setting STOPPED is unsupported. Separate render-queue-item suites remain
unavailable; no item or queue operation reports pretend success. The existing
suite catalog and lease tracker own acquisition/release; no new lease mechanism
or execution enforcement tier is introduced.

UtilitySuite3 (acquisition version 7) already supplies RegisterWithAEGP. Its
existing plugin ID 1 works for this library; no registration behavior was changed.
The native selftest calls that slot with the library's null first argument.
NF global setup successfully acquires/releases both suites.
Reference: [NF_AE.h GlobalSetup](https://github.com/bryful/F-s-PluginsProjects/blob/71527540a201c1cc7ad41fa6acdab0d2f7a99fab/_NFLib/NF_AE.h#L473).

## Reproduction and results

Cohort: all 44 `.aex` files in the public MIT NF-Plugins_20260504 distribution;
no recursive/configured-machine scan. Binaries and private paths are not included.
Input: `corpus/issue279-20261005/input.png`, 256 x 144,
PNG file SHA256 `59dbddbbae1e3d3266f9a85a57f3d593bec2c7f8f7f12ea3a6a0c409bdebd753`.
The decoded argb8 input payload hash in the render report is
`afcfb632481c5b6ab25eebdaa14791209a108c5b2b4163bcd3709a3c6e6997d4`;
the higher-depth payload hashes differ from it.
Default parameters, one frame, time=0, total=1, scale=30:

```
aexcompat-harness --headless --render-experimental-session <aex> corpus/issue279-20261005/input.png <out.png> <argb8|argb16|argb32f> <classic|smart> 0 1 30
```

Before: main `9a7f25e0e`, rebuilt native and harness. All 44 failed both argb8
routes (88/88 parameter-inspection errors). The initial issue identifies the
GLOBAL_SETUP=516 / missing Render Queue Suite cause; the baseline harness command
returns the redacted `inspection_error` / `parameter_inspection` diagnostic.
Higher-depth before-runs were not repeated. After: the same base plus this PR,
rebuilt native/harness; all 264 combinations were executed. No fallback or
failed-run promotion was used.

| Failure/result group | Before 8 classic | Before 8 smart | After 8 classic | After 8 smart | After 16 classic | After 16 smart | After 32f classic | After 32f smart |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| GLOBAL_SETUP / inspection failure | 44 | 44 | 0 | 0 | 0 | 0 | 0 | 0 |
| Render passed | 0 | 0 | 43 | 42 | 43 | 42 | 42 | 42 |
| MuzzleFlash: partial_unwritten_output (#1838) | — | — | 1 | 1 | 1 | 1 | 1 | 1 |
| ColorSampler: smart worker exit 22 (#1839) | — | — | 0 | 1 | 0 | 1 | 0 | 1 |
| CellLineEraser: classic float copy/iterate 516 (#1840) | — | — | 0 | 0 | 0 | 0 | 1 | 0 |

All 44 pass global setup. 254/264 renders pass with balanced suite leases,
intact output guards and matching plugin identity in the success report.
MuzzleFlash also reports balanced leases on its rejected output. ColorSampler
has no successful smart frame report; its diagnostic records successful global
setup/setdown and no missing or unsupported suites, not a successful render.

- [#1838](https://github.com/onmokoworks/AEXCompat/issues/1838): MuzzleFlash leaves
  about 42.5% of the output unwritten in both routes at every depth.
- [#1839](https://github.com/onmokoworks/AEXCompat/issues/1839): ColorSampler succeeds
  in classic at every depth but fails the explicitly requested smart route with
  pre_error/render_error=-1 and exit 22. Route suitability remains to investigate.
- [#1840](https://github.com/onmokoworks/AEXCompat/issues/1840): CellLineEraser fails
  only float classic: copy/pixel_format_mismatch and iterate/missing_world, 516.

These are observations, not AE image-equivalence claims. No AE oracle was run.
No implementation of these follow-ups is included.

## Build identity

- Before worker SHA256: `f012aed62cb18bfb25b75fc2cbbcbf4cd3c3fb94582384b00597e9d0bf88cb18`
- Before harness SHA256: `5a9df8570c1261635f4f81806280a7662269ed3dbc52f85248cc3409c2e7fdd5`
- After worker SHA256: `71e290266b03b8d5ae8c2399e0c4a3ea54e98ea5d210bd9b0ee0ff4e96e6c684`
- After harness SHA256: `67bbc2545bd09e038cd0e141bb436f82e73cc9a318e914d860dd978dd79e1f78`

The harness was rebuilt again by the workspace test run between the cohorts.
Both harness builds use unchanged harness source; hashes identify the actual files.
Raw reports and small output images are local-only under `target/fs-out`.

## Validation

- `tools/build-native.ps1`: Release build passed; all 224 worker translation units
  have recorded header dependencies. Existing build directories were reused.
- `PYTHONUTF8=1`, `AEXCOMPAT_CANONICAL_WORKER=<existing worker>`:
  `uv run python -m pytest -q tests/test_render_queue_suite.py tests/test_suite_entry_utility13.py tests/test_world_reclamation.py --run-sdk-tests --run-built-artifact-tests`
  — 9 passed, including three worker routes and the external SDK ABI compile.
- `worker_suite_registry_selftest.exe` — passed;
  `uv run python -m pytest -q tests/test_worker_dynamic_suite_registry.py tests/test_artifact_manifest_validation.py --run-built-artifact-tests`
  — 2 passed.
- `uv run python -m pytest --collect-only -q --validate-local-artifact-manifest`
  — 1418 collected, manifest valid. Three parameterized native cases are in
  `built_artifact_tests.txt`; ABI compile is in `sdk_required_tests.txt`.
- `cargo test --manifest-path broker/Cargo.toml --workspace -- --skip native_cluster_swap_records_each_members_depth_and_final_flags --skip native_launch_validation_failure_reaches_broker_without_values_or_paths`
  — 817 passed test executions, 16 ignored; two named native cases excluded.
  The initial unfiltered broker test run failed those two because the sampling
  and catalog probes are absent. No extra target directories were created to
  build those fixtures. CI provisions them; this local run does not verify them.
- Full Python execution was not run locally because the broad suite requires
  additional fixtures/AE resources; CI runs the artifact and SDK partitions.

The selftest acquires v1 twice through the production host, rejects unknown v2,
checks the actual state output and null-pointer error, calls both mutators
(including all defined states and UINT32_MAX), rechecks unchanged state,
exercises UtilitySuite3 registration, and releases all three references even
on failed assertions. Pytest checks the observed slot/count diagnostics and
lease deltas. It does not search product source strings.
