# Security Policy

## Reporting a vulnerability

Please use GitHub's private security advisory feature. Do not open a public
issue for a vulnerability that could expose a secret, a local path, or a
machine-specific artifact.

Reports must not include proprietary AEX plug-ins, Adobe SDK files, DLLs,
process dumps, private corpora/assets, credentials, or personal filesystem
paths. Provide a minimal synthetic reproducer and redacted diagnostics instead.

## Security boundary

AEXCompat runs untrusted native plug-ins out of process with Windows Job Object,
restricted-token, bounded-output, memory-limit, and staged-file controls. This
is crash and integrity containment; it is not a confidentiality sandbox. Treat
every AEX as untrusted and use a disposable machine or VM for sensitive work.
