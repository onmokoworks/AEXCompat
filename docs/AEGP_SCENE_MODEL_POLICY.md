# AEGP scene model policy

This policy defines the bounded authored-scene contract used by the three
production workers. It covers Issue #26 scene identity and scheduling. Mask
pixel semantics remain owned by Issue #29 APIs, and parent/camera/zoom math
remains owned by Issue #30 APIs.

## Identity and ownership

- Projects, items, compositions, folders, footage, layers, effects, streams,
  values, and keyframes have registry-owned typed identities:
  `(project_id, object_id, generation, kind)`.
- Scheduler callers provide typed identities, never a pointer plus an
  independently supplied numeric ID. The scheduler derives its private key and
  stable hash from the exact live registry record.
- An item dependency must be a live composition item in the same project.
- An effect must be live, owned by a layer in the item's composition, and
  listed exactly once at its current contiguous stack order. Duplicate
  `(layer, stack_order)` positions are rejected before scheduler or receipt
  publication; the previous graph and live receipts remain unchanged.
- ABI calls without a plug-in ID use the existing explicit possession policy:
  only an exact live registry token with the required kind is accepted.
  Unsupported suite slots remain unsupported and record their suite, version,
  slot, and call count through the suite diagnostic path. Invalid handles
  return the ordinary invalid-parameter error and increment a separate
  invalid-handle diagnostic.

## Generations and invalidation

- Object generations never wrap. A stale object identity does not resolve.
- Render project generations increment exactly once after a successful atomic
  mutation and do not change on validation failure or cancellation.
- Before that non-failing generation increment, the transaction invalidates
  every typed scheduler stage and receipt from the old project generation.
  Receipt invalidation also unregisters its borrowed world, so a later use is a
  distinguishable invalid-handle operation and cannot expose stale pixels.
- Stage hashes include the typed project/item/effect identities, object
  generations, effect order, render generation, time, quality, and output
  shape. Receipt trace evidence additionally commits to dependency traversal
  and ordered effect boundaries.

## Transactions

Scene mutations use `AtomicSceneTransaction`:

1. begin and capture registry fingerprint plus project generation;
2. stage bounded candidate state;
3. validate identity, ownership, capacity, and mutation-specific rules;
4. apply atomically;
5. invalidate old typed stages and receipts;
6. increment the render project generation once; or
7. cancel without changing live scene bytes or project generation.

Failure and cancellation publish no receipt and leave no reservation or
borrowed-handle capacity behind.

## Scheduler graph

- Registration is bounded to 16 items, 8 direct dependencies per item, and 8
  effects per item.
- Duplicate stable identities, duplicate dependencies/effects, wrong-kind or
  foreign identities, cross-project edges, non-contiguous effect order, and
  duplicate `(layer, stack_order)` positions or pointer/identity mismatches
  fail before state publication.
- Direct and indirect dependency cycles are rejected while staging the
  registration update. The previous graph remains live and unchanged.
- Resolution retains the existing depth, stage-count, time, memory, and
  immutable-world limits from Issue #25.

## Required local evidence

`--self-test-aegp-scene-model` must pass on `aex_l2_worker`,
`aex_render_worker`, and `aex_smart_worker`. Its strict JSON report is validated
by `schemas/aegp-scene-model-selftest.schema.json`, rejects duplicate keys,
requires non-zero identity/dependency/order/trace hashes, and requires zero live
or reserved receipts. The test invokes a published unsupported Effect Suite
slot and requires the observed suite diagnostic/error to remain distinct from
invalid-handle rejection. It also injects a duplicate effect position and
requires rejection with unchanged scheduler state and receipt evidence.
Existing scene mutation, staged-item, mask, camera, and 3D self-tests remain
independent gates. Real-AEX probing is a later phase and is not represented as
completed by this self-test.
