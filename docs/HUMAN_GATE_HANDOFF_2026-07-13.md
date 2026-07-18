# Human Gate Handoff (2026-07-13)

> **Historical gate record:** this handoff captures the state before the owner
> authorized native execution. Its closed-gate statements are superseded by
> `SAFETY_GATE_STATUS_2026-07-13.md` and the continuing authorization record.
> Retain it for audit history, not as current execution policy.

This document requests human input. It is not an approval, receipt, legal
conclusion, ABI decision, or Safety Gate opening declaration. Do not treat
filled placeholders, test artifacts, or an agent-generated file as evidence of
human authorization.

## H-1 Fixture Provenance And Decision

Candidate currently on hold: `AEPluginBuild\ScatterMap.aex`.

The repository owner must answer all of the following in their own words:

1. Origin: how was this exact candidate obtained or built?
2. Rights: what permission allows private compatibility testing of it?
3. Scope: is use restricted to this machine/project, or is redistribution permitted?
4. Identity authorization: may a human-reviewed tool read the file solely to record SHA-256 and byte size?
5. Fixture decision: `hold`, `reject`, or `approve_for_separate_loader_gate_preparation`?

An approval decision does not authorize loading, executing, rendering, calling
an entry point, or opening the Safety Gate. It only allows preparation of a
separate, expiring loader receipt after dependency and legal review.

## H-2 AE SDK And License

The repository owner must obtain the SDK outside Git and record:

- SDK product/version and local root (the path remains local-only);
- source and acquisition date;
- whether the license permits compiling the instrument samples;
- whether headers, ABI definitions, binaries, or derived code may be committed;
- reviewer name/date and any redistribution restrictions.

The result belongs in `analysis/AE_SDK_LICENSE_NOTE_<date>.md`. Until that note
exists, SDK-dependent builds are unverified and no SDK material may enter Git.

## H-3 ABI Provenance Choice

Choose one boundary and document the rationale in
`docs/ABI_PROVENANCE_DECISION_<date>.md`:

1. Public-document cleanroom: minihost implementers use public documentation,
   independently designed contracts, and black-box traces only. SDK-aware
   instrument code remains isolated under `instruments/`.
2. SDK-derived ABI: permitted only after a human/legal review identifies terms
   that explicitly allow the intended implementation and redistribution.

The public-document cleanroom boundary is the safer default, but this template
does not make the decision. Whichever option is selected must define who may
read SDK material and how information is prevented from crossing into
`minihost/`.

## Required Reply

The next useful human response should provide the five H-1 answers, state
whether an AE SDK has been obtained, and select an H-3 boundary. Until then,
Phase D and H-4 remain closed.
