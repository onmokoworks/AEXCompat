# AEX Host Core Boundary

AEXCompat targets observable compatibility for AEX plug-ins. ScatterMap is the
first owner-authored conformance fixture, not the product architecture or the
set of plug-ins the host intends to support.

## Host Core

`broker/crates/broker/src/host_core` contains reusable host policy and behavior.
It may define parameter kinds, descriptor models, validation, selector policy,
world and suite contracts, process isolation interfaces, and generic reports.
It must not contain a fixture name, fixture hash, fixture parameter label,
fixture output hash, or fixture algorithm.

## Fixture Profiles

`broker/crates/broker/src/fixture_profiles` contains explicit local conformance
adapters. A profile may bind discovered descriptors, a reviewed allowlist id,
and an independent oracle used to prove compatibility. Profiles are test
evidence and safety configuration; they do not define generic host semantics.
Unknown profile ids fail closed and are never interpreted as ScatterMap.

The current ScatterMap profile owns a typed render adapter and ARGB8 oracle. Its
parameter definitions are not compiled into that adapter: a promoted L2
descriptor manifest records all seven observed slots, marks five numeric slots
assignable, binds the observation to the reviewed plug-in digest and L2 receipt,
and is pinned by a canonical JSON SHA-256 in the registry. The generic manifest
loader owns strict parsing, digest verification, slot continuity, uniqueness,
type/kind agreement, and finite range/default checks. The generic parameter core
owns descriptor-id lookup, validation, default application, and worker payload
encoding. Render request schema v2 carries a descriptor-id keyed numeric map.
Parameter validation and render CLI operations are fixture-neutral and resolve
`plugin_id` through the registry. Allowlist parsing and artifact/resource
validation are generic host policy; each profile supplies its reviewed approval
policy and worker launch specification. Parameter workers receive one bounded, versioned
descriptor/slot/kind/value payload rather than a fixed positional argument list.
They validate its syntax before native loading, then match every requested slot,
kind, and range against descriptors observed during `PF_PARAMS_SETUP` before any
render selector. Classic and SmartFX build variable-length `PF_ParamDef` storage
from those descriptors and apply observed defaults consistently. The broker-side
typed render adapter, legacy fixed-case commands, and concrete allowlist records
remain fixture-specific migration debt.

## Completion Rule

The host cannot be called general solely because ScatterMap passes. General AEX
support requires at least:

1. descriptor-driven request values rather than a fixed five-field structure;
2. generic PiPL and entry-point discovery feeding host registration;
3. generic parameter storage and selector dispatch independent of fixture names;
4. profile-independent image/world and suite implementations;
5. a second owner-authored AEX profile passing load, setup, render, and failure
   isolation without changes to `host_core`.

Until those conditions are met, reports must describe ScatterMap as the active
fixture and must not describe AEXCompat as a complete general AEX host.
