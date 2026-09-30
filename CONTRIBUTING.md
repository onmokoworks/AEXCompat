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

AEXCompat-authored source and documentation are contributed under MPL-2.0.
This does not change the terms of third-party dependencies, Adobe SDK material,
separately licensed fixtures, or private/commercial corpus files. Confirm the
provenance and applicable upstream terms before copying or redistributing any
such material. A guest executable combined with Unicorn must additionally meet
the GPL-2.0 distribution conditions described in the README and public-release
audit.
