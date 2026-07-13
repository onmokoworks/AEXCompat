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
policy and worker launch specification. L2 lifecycle validation is also generic:
the profile registry owns receipt, expiry, About identity fragments, advertised
flags, and conditional-selector expectations, while the runner applies the same
setup/setdown, no-render, isolation, and report rules to every registered id.
Parameter workers receive one bounded, versioned
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
render fixture and must not describe AEXCompat as a complete general AEX host.

MaskOffset is now registered as a second observation fixture and passes the
generic L1/L2 runner without `host_core` changes. Its masked SmartFX render also
passes through generic AEGP Utility, PF Interface, Layer Mask, Stream, and Mask
Outline suites with an independent pixel oracle. The host implements the
`pre_render_data` transfer/delete lifecycle and PF World pixel-format query;
suite dispatch depends only on suite name/version and opaque host handles.
Classic render remains explicitly unsupported and is rejected before native
launch. Descriptor/request/worker v3 now carries strict ARGB8 Color values and
MaskOffset's custom Fill Inside output passes an independent pixel oracle.
The fixed suite-fault route additionally proves mask callback error fallback and
Access Violation isolation without accepting arbitrary fault modes or native
paths. This is evidence toward item 5, not completion of it: configurable mask
scenes and wider selector/suite coverage remain pending. Host-owned mask records
now support zero, one, or multiple distinct mask/stream/outline handles, with
fixed scene selection gated outside the ABI callback layer. Request v4 also
accepts bounded open/closed cubic-Bezier host context with relative tangents;
broker and worker both validate its shape and limits before exposing handles to
native code. The fixture-side independent oracle now also covers corner
rounding, uniform and separate-X/Y expansion, feathering, inversion, and a
combined typed-color render. These algorithms remain outside `host_core`; the
generic host only transports descriptors, values, worlds, and bounded mask ABI
state.

Opaque mask, stream-reference, and stream-value handles now have explicit
single-owner live states. The worker rejects duplicate disposal and refuses to
dispose a stream while a checked-out value remains live. Broker conformance
requires one acquire/dispose pair for each object used by a host-context render
and verifies that no object remains live. Fixed rejection tests restore valid
state after the expected AE error and remain unavailable to arbitrary requests.

SPBasicSuite acquisition is tracked by exact suite name and version under a
thread-safe reference count. Only successful acquisitions create a lease, and
release of an unknown or zero-count key returns an SP error without changing
state. Reports retain total acquisitions, releases, live keys, and live
references. MaskOffset ends with all AEGP leases released but one
`PF Handle Suite@2` reference retained by the dependency's raw-handle ownership
transfer; this is recorded as a bounded module-lifetime lease rather than
misreported as balanced. The broker forbids over-release and requires the live
reference count to equal acquisitions minus releases.

PF Handle records separately track allocation, lock depth, size, and disposal.
The callbacks are mutex-protected; unknown handles and unmatched unlocks are
recorded, and resize or dispose while locked is rejected without invalidating
the handle. Native MaskOffset evidence creates and disposes one global-data
handle, performs six balanced lock/unlock pairs, and ends with zero live handles
and zero invalid operations. This remains distinct from the retained suite
lease above. A fixed fault locks a temporary handle, verifies resize rejection,
then unlocks and disposes it to restore a fully balanced state.
The worker additionally caps the complete handle pool at 1024 records and
64 MiB total, so repeated individually valid allocations cannot bypass the
per-handle memory bound. Resize is checked against the same aggregate budget.

PF World Suite v2 is now a generic host-core service rather than a fixture
adapter. Its local cleanroom ABI declaration follows the reviewed public SDK
contract: NewWorld, DisposeWorld, then GetPixelFormat. NewWorld accepts explicit
ARGB32, ARGB64, and ARGB128 formats and constructs the complete 120-byte x64
PF_EffectWorld layout, including WRITEABLE/DEEP flags, data, rowbytes,
dimensions, full extent, and square pixel aspect. Smart checkout worlds remain
borrowed and cannot be disposed through this ownership path.

Each created scratch world has a mutex-protected ownership record containing
its exact format and allocation size. The host permits at most 64 live worlds
and 256 MiB total, rejects unsupported formats, invalid dimensions, overflow,
duplicate creation into a live world, borrowed disposal, and double disposal.
Rejected allocations do not alter the caller's world. Non-cleared worlds use a
deterministic poison fill rather than exposing stale allocator memory.
