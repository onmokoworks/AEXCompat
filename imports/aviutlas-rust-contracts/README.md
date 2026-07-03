# AviUtlas Rust Contract Import

This directory collects AEX/AEPX/OFX compatibility planning assets copied from
`D:\Projects\01_Project\05_other\AviUtlas`.

The import preserves the original relative paths so the material can be audited
against its source without mixing it directly into the Python-first AEXCompat
tool layout.

## Imported Areas

- `analysis/`: AEX/AEPX/OFX schemas, strategy notes, tool specs, and handoff
  documents.
- `aviutl-rs/examples/`: Rust command examples for no-load AEX/AEPX/OFX gates
  and reports.
- `aviutl-rs/tests/`: Rust contract tests and fixtures for the same boundary.

## Cleanup Policy

This is a staging import, not yet the canonical implementation layout. Prefer
promoting useful behavior into `tools/` and `tests/` as focused Python lab
tools, while keeping this directory as provenance until the migration is
reviewed.
