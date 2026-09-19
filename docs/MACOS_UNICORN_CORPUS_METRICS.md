# macOS arm64 Unicorn corpus metrics

`tools/sweep_macos_x64_aex.py` measures the Windows x64 AEX corpus that is
available on a Mac through the arm64 Unicorn correctness backend. The normal
invocation selects only Unicorn. It does not launch a Windows machine, VM,
Wine, a Windows-built worker, Rosetta, or the optional native carrier.

The denominator is the complete set of unique Mac-local AEX SHA-256 identities
that map exactly to the supplied canonical Windows inventory. Missing,
duplicate, non-x64, or unbound identities stop the run. Entry order is the
SHA-256 order and durable reports contain no absolute plug-in paths.

## Metrics

Schema version 2 records exact numerator/denominator pairs rather than rounded
percentages for these cumulative milestones:

- `admission_success`: setup and the disposable admission render completed;
- `render_success`: a fresh process produced a validated minimal frame;
- `cleanup_success`: the rendered session completed clean setdown and exited.

Each failure retains its first known stage and a bounded failure class. Typed
resident diagnostics additionally retain the selector and bounded suite
evidence. The summary groups those blockers without turning a descriptor-only,
crash, timeout, unsupported import, or cleanup failure into success.

## Baseline comparison

Pass `--baseline-report <schema-v2-report.json>` to compare two runs. Inventory,
Windows summary, input image, dimensions, selected backend list, parallel job
count, and mapped SHA order must match exactly. Worker SHA values may differ so
a new implementation can be measured; both identities are retained in the
comparison.

The command exits nonzero after retaining the report if a previously rendered
identity stops rendering, a previously clean identity stops cleaning up, or a
successful output SHA changes. Newly rendered identities are reported as gains.
Condition or identity drift is rejected before comparison.

Example (paths and canonical SHA values are operator-supplied):

```sh
python3 tools/sweep_macos_x64_aex.py \
  --inventory <canonical-inventory.json> \
  --windows-summary <canonical-summary.json> \
  --corpus-root <sha-addressed-aex-directory> \
  --input-png <control.png> \
  --unicorn-worker guest/target/release/aex-guest-worker \
  --jobs 4 \
  --output <current-report.json> \
  --baseline-report <previous-report.json> \
  --expected-inventory-sha256 <sha256> \
  --expected-summary-sha256 <sha256>
```

`--jobs` controls how many isolated worker processes run concurrently (default
4, maximum 32). Report entry order remains the deterministic SHA-256 order;
parallel completion order never changes the durable report. Each AEX/backend
attempt keeps its own process group and run directory. Runner-initiated cleanup
therefore targets only that attempt, and results from completed attempts remain
in the final report when another worker crashes or times out.

The Rosetta native carrier remains an explicit comparison-only choice via
`--backend native --native-worker <path>`; it is not part of the default path.
