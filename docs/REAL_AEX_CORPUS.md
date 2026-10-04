# Real Effect AEX corpus and triage

Issue #9 extends the Issue #4 conformance contract across five exact AEX
identities from three suppliers. `corpus/real-aex-public.json` contains only
public provenance, licensing, redistribution status, byte size, and SHA-256.
The binaries are never committed, persisted in public output, or attached.
They are copied only into a temporary local Issue #4 execution bundle, which
is deleted immediately after validation.

Create `corpus/local-locator.json` locally according to
`schemas/real-aex-locator.schema.json`. This ignored file is the sole mapping
from a public corpus id to an absolute installed path. The orchestrator hashes
every AEX before launch and rejects a missing, substituted, linked, or
wrong-sized file. It never writes locator paths into its report.

`corpus/common-matrix.json` explicitly crosses Classic and SmartFX with
ARGB8/16/32F, typed parameter sets, and rational times. Every cell is executed
in-process through the Issue #4 `run-conformance-bundle.py` runner and validated
against the same manifest/report schemas. Its result is normalized into exactly one of:
`ok`, `loader_error`, `selector_error`, `missing_suite`,
`host_validation_error`, `crashed`, or `timeout`. An absent Effect entry point
is always `loader_error`; downstream evidence cannot mask it.

Run locally:

```powershell
python tools/run-real-aex-corpus.py `
  --inventory corpus/real-aex-public.json `
  --locator corpus/local-locator.json `
  --matrix corpus/common-matrix.json `
  --runner broker/target/release/aexcompat-harness.exe `
  --input target/local-corpus-input/control.png `
  --out target/real-aex-corpus/run-001 `
  --private-evidence-out target/real-aex-corpus-private/run-001
```

The local source AEX and declared dependencies are first copied through the
Issue #4 single-handle identity boundary into a private temporary source tree;
dependency basenames are preserved exactly for PE import resolution. Each
temporary bundle binds the AEX, dependencies, input, harness, and the single
native worker, is fully validated, and is deleted before publication.
Discovery, Classic, and SmartFX select routes within that worker; they are not
three executable identities. Current triage and public gap evidence require
exactly one `worker-000` identity, with `schema_version: 2` validated by
`real-aex-triage-v2.schema.json` and `real-aex-public-evidence-v2.schema.json`.
The original v1 schemas retain the three-worker contract for archived packets.
Do not duplicate a hash into three role aliases or rewrite historical packets.

The private-local triage report retains corpus identities and exact Issue #4
manifest/report diagnostics for auditability. It is never an attachment and
must not be committed. The public output contains only structurally validated,
non-joinable gap shapes and sanitized host identities.
`reproducible-gaps.json` deliberately contains no AEX id, AEX hash, local
path, parameter name, or parameter value. It aggregates missing Suites by the
number of distinct AEX SHA-256 identities under a stable hashed Suite id, never by retry/case count, so it can
be attached to a general compatibility Issue without redistributing or naming
a proprietary plug-in.

Each grouped public replay uses `--case-list <PRIVATE_CASE_LIST>`. Create that
private JSON array from the matching `gap-map.json` `case_ids` entry; the runner
then selects every occurrence in the group and rejects empty, duplicate,
malformed, or unknown case ids. The exact case list remains private-local.
