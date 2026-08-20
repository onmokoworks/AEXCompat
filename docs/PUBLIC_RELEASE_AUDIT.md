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
| `broker/` Rust workspace | MIT in Cargo workspace metadata | Blocked until repository license/provenance is approved |
| `guest/` Rust workspace | MIT in Cargo workspace metadata | Blocked until repository license/provenance is approved |
| AviUtl2 and YMM4 Rust bridges | MIT in Cargo package metadata | Blocked until repository license/provenance and dependency notices are approved |
| `minihost/`, Python tools/tests, docs, schemas, instruments | No root license grant found | Blocked: owner/legal decision required |
| Adobe SDK-backed probes/fixtures | Adobe-proprietary SDK dependency | Source boundary and redistribution terms require manual review; SDK is never exported |
| Third-party Cargo/Python dependencies | Upstream licenses | Generate and manually review a locked dependency notice/SBOM before publication |

No `LICENSE` file is added by this work because choosing one would be a
relicense decision. The owner must approve the repository-wide grant and every
component exception before publication.

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
