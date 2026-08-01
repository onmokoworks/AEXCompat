# Contributing

Contributions should be based on public documentation, clean-room observations,
and synthetic fixtures. Keep compatibility behavior and byte-exact outputs
covered by focused tests.

Before opening an issue or pull request:

1. Run `PYTHONUTF8=1 uv run python -m pytest -q` on Windows.
2. Run the affected Rust tests and Release worker self-tests when applicable.
3. Confirm that generated JSON/manifests have no duplicate keys, private paths,
   or machine-bound identities.
4. Keep SDK-, oracle-, corpus-, and locally built artifact tests opt-in.

Never submit proprietary AEX plug-ins, Adobe SDK files or excerpts, DLLs,
dumps, private assets/corpora, credentials, license keys, personal paths, or
unredacted host diagnostics. A hash of a private binary can itself be sensitive;
use a synthetic identity unless maintainers explicitly approve publication.

The repository's overall public license is not implied by a component's package
metadata. Do not copy, relicense, or redistribute a component until its license
and provenance are documented for the public release.
