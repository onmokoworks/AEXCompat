# Public release audit

This document records the minimum publication boundary. It is not publication
approval and does not authorize a push or a license change.

## Current findings

- The tracked `main` tree contains no `.aex`, `.dll`, `.exe`, `.pdb`, `.dmp`,
  `.dump`, archive, AEP, or PSD payload found by the initial extension audit.
- Git history includes author/committer addresses derived from a Tailscale host
  name and a `.local` host. The disposable export rewrites those addresses to a
  public noreply address.
- Local and remote development refs include many Codex/agent branches. They are
  not export inputs. Only `main` and tags named via repeated `--tag` arguments
  are retained.
- The private SDK asset tag `ci-sdk-ae25.2` exists. It must not be selected for
  a public export without a separate provenance and license decision. CI no
  longer reads it: the SDK zip now comes from the private R2 bucket
  `aexcompat-ci` (#1445), which must likewise stay non-public. Publishing the
  bucket, or moving the object behind an unauthenticated URL, would redistribute
  the Adobe SDK and is a separate license decision, not a CI convenience.
- Historical and current-tree secret/prohibited-file scans remain mandatory
  after rewriting. A finding blocks export; it is never converted to success.
- Historical Python bytecode was found to contain an absolute personal path;
  `.pyc` and `.pyo` are therefore removed from all exported history.

## Component license inventory

| Component | Declared metadata | Publication state |
| --- | --- | --- |
| AEXCompat-authored source, docs, schemas, tests, and instruments | MPL-2.0 root grant and Cargo metadata | Source grant resolved; dependency notices/SBOM and provenance review still gate publication |
| `guest/` AEXCompat-authored Rust source | MPL-2.0 | For a distributed GPLv2 Unicorn Larger Work, relevant Covered Software must additionally be distributed under GPL-2.0 and the combined work must satisfy applicable GPLv2 terms |
| `imports/` and provenance-identified copies | Upstream/source terms | Not relicensed by the root grant; independent provenance approval required |
| Adobe SDK-backed probes/fixtures | Adobe-proprietary SDK dependency | Source boundary and redistribution terms require manual review; SDK is never exported |
| Third-party Cargo/Python dependencies | Upstream licenses | Generate and manually review a locked dependency notice/SBOM before publication |
| Private/commercial AEX corpus | Upstream proprietary or per-artifact terms | Local testing only; never included in a public export |

The root `LICENSE` grants MPL-2.0 only for material the AEXCompat contributors
have the right to license. It does not replace upstream notices or authorize
redistribution of excluded SDK, corpus, fixture, dependency, or generated
material. The AEXCompat MPL files are not marked incompatible with secondary
licenses. Publication remains blocked until the dependency notice/SBOM and
artifact-provenance gates are separately approved; a distributed Unicorn-linked
guest also requires the relevant AEXCompat Covered Software under both MPL-2.0
and GPL-2.0, plus applicable notices and corresponding source/build information
for the combined work.

## Dry run

Start from a fresh private clone with no local-only artifacts:

```powershell
$env:PYTHONUTF8 = '1'
uv run python tools/public_export.py --source C:\path\to\fresh-private-clone `
  --out C:\path\to\create-new-public-export
```

Add only a reviewed public tag with `--tag vX.Y.Z`. The tool has no push
operation, removes the source remote, rejects unexpected refs, normalizes
host-derived emails, removes prohibited binary extensions from disposable
history, and scans all reachable exported blobs. Never use `git push --mirror`.

Human publication steps are: resolve licensing, review scan findings and every
selected tag, create a new empty public repository, configure public CI and
security settings, and push only the reviewed `main` plus individually approved
tags using explicit refspecs.
