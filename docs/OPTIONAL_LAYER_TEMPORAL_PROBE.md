# Optional-layer temporal checkout probe

The SDK-built optional-layer probe requests its primary at the current render
time and its map at `current_time + offset * time_step`. It records the map's
PreRender result and subsequent pixel checkout in the existing 32-word image
payload. A failed map checkout is recorded rather than hidden by a failed frame.
This permits the output image to distinguish callback failure from successful
retrieval of actual supplied pixels.

Build the default current-time probe and the one-step future variant:

```powershell
./tools/build-pf-empty-layer-contract-probe.ps1
./tools/build-pf-empty-layer-contract-probe.ps1 -MapTimeOffset 1
```

Offsets are integers from -5 through 5. Nonzero variants use separate build
directories so they cannot replace the default artifact. The request arithmetic
uses 64-bit intermediates and records an invalid-argument result if the requested
time is outside the SDK time type. An offset of zero retains the original schema
and current-time behavior. The requested offset is a build condition, not a new
payload field; record the variant and actual artifact identity with observations.

`tests/test_pf_empty_layer_contract_probe.py` decodes current/future observations
for an unassigned map and an explicitly supplied, time-invariant secondary image.
The broker integration test
`optional_future_checkout_distinguishes_absent_and_missing_timed_input` additionally
supplies a timed map at a missing or matching timestamp. It checks the decoded
callback errors, absence of a pixel checkout after failed PreRender, exact supplied
pixels on success, and the worker's output/ownership health conditions.

The current host returns an empty result and a transparent world for an unassigned
map at the current time, but refuses its future checkout. It serves explicit still
images at future times. A timed source without the requested frame is refused;
a matching frame is served without substituting current-time pixels.

These tests characterize AEXCompat, not After Effects. They do not establish what
AE returns for an unassigned layer at another time, or prove that the existing
transparent-world representation matches AE. Do not move the temporal guard or
reinterpret missing timed footage as an empty layer solely from these observations.
Independent contract evidence is needed before changing that behavior.
