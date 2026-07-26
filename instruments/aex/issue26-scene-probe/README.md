# Issue #26 public-AEGP scene probe

This probe is one public-SDK AEGP binary used unchanged under After Effects and
AEXCompat. It contains no host detection. The host-independent input is the
`ISSUE26_SCENE_PROBE_EVIDENCE` output path.

The probe enumerates public project/item/comp/layer/effect/stream identities,
records effect and stream order, observes parent/camera/zoom and keyframe
metadata, attempts public batch-keyframe cancel/commit, and verifies that a
stream borrowed from a duplicated effect is rejected after that owner is
deleted. Every unavailable suite or operation is emitted with suite, version,
slot, operation, and error. Pixel equality is deliberately outside the oracle.

`fixture.jsx` authors the real-AE scene. It uses only ExtendScript project APIs
and does not alter the probe behavior.
