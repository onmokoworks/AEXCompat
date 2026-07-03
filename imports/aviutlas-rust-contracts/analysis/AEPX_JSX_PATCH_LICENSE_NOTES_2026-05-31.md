# AEPX / JSX Patch License Notes - 2026-05-31

Purpose: record dependency and publication cautions for the AE project-editing
slice.

## Current Position

The first contract slice should not require new third-party code.

Already available in `aviutl-rs`:

- `serde`
- `serde_json`
- `anyhow`
- `thiserror`

## AEPX XML Library Caution

The `.aepx` patcher needs stronger preservation behavior than a normal
read/modify/write XML model. Before adding an XML dependency, verify:

- license compatibility;
- preservation of comments;
- preservation of CDATA;
- preservation of namespace prefixes;
- preservation of unknown attributes;
- whitespace behavior;
- whether writing canonicalizes or pretty-prints the whole document.

If preservation cannot be guaranteed, v0 should fail before writing rather than
silently normalize a private project.

## JSX Tooling Caution

Generated JSX can be produced as plain text from JSON data. Typed JSX tooling or
bundlers can be considered later, but v0 should not add:

- arbitrary JavaScript execution;
- shell commands;
- AE runtime launchers;
- network-capable build steps;
- minified generated scripts that are hard to audit.

Previously observed candidate tooling such as Types-for-Adobe or AE JSX
bundling tools still needs exact license/source review before import.

## Publication Boundary

Do not publish or commit:

- local `.aep` files;
- local `.aepx` files;
- local `.ffx` presets;
- generated files that embed private project XML or media paths;
- screenshots/logs that expose private project contents.

Synthetic fixtures and independently written schemas are safe candidates for
public use after normal review.
