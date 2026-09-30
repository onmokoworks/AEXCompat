# macOS OpenFX → AEX one-frame execution (#385)

`tools/openfx_macos_session.py` connects a host-neutral RGBA8 OpenFX frame to
the existing macOS fixture harness and resident guest worker. It reuses the
validated AEX render and artifact checks used by the Blender adapter, then
returns a packet conforming to
`contracts/openfx/render_session_bridge.schema.json`. This is an actual AEX
worker render when the response status is `rendered`; it is not an After
Effects equivalence claim.

Set `AEXCOMPAT_PLUGIN_ROOT`, `AEXCOMPAT_HARNESS`, and
`AEXCOMPAT_GUEST_WORKER` to a legally obtained plug-in tree and locally built
Release binaries. The caller sends one JSON object to stdin and receives one
bridge packet on stdout:

```json
{
  "plugin_relative_path": "effect.aex",
  "width": 1,
  "height": 1,
  "rowbytes": 4,
  "pixels_base64": "AAAA/w==",
  "frame_index": 0,
  "current_time": 0,
  "time_step": 1,
  "total_time": 1,
  "time_scale": 1000,
  "alpha_mode": "straight",
  "render_path": "classic"
}
```

Use `uv run python tools/openfx_macos_session.py < request.json > response.json`.
Exit code 0 means a verified `rendered` packet, 1 means a fail-closed bridge
packet, and 2 means the request was rejected before a worker launch. JSON
duplicate keys, non-finite values, unknown fields, invalid base64, oversized
payloads, root escapes, and unsupported geometry fail before execution.

The first Mac slice accepts one RGBA8 frame with straight alpha, positive
pixel-aligned rowbytes, Classic or SmartFX, and millisecond frame time from
0 to 3600 seconds. `time_step=1`, `time_scale=1000`, and
`total_time=max(1,current_time)` reflect the underlying fixture timing. Host
row padding is removed before the AEX worker; the bridge request records the
original padded bytes and the response contains packed RGBA8 rows. Each call
opens, renders, and closes its own fixture session. A persistent multi-frame
OpenFX session and premultiplied/deep formats require a later slice.

The worker's artifact report, input checkpoint, output geometry and checksums
are validated by the existing macOS render path. The adapter checks the
observed plug-in, harness, and worker identities again after the run. Changed
file identity or a file lost during the backend run yields
`identity_mismatch` without output; timeout, worker crash,
and malformed artifacts also return packets without output. A generic nonzero
harness exit is classified as `unsupported` with `worker_failure`; it is not
asserted to be a crash. Hashes qualify
the recorded result and do not block launching a changed plug-in.

## Local real AEX evidence

On Apple Silicon macOS, locally built Release harness and guest worker ran
the legally held, license-free `DistanceGradation.aex` through this adapter.
The generated 64×64 straight-alpha RGBA input used 260-byte host rows with
four padding bytes and frame time 250 ms. The AEX worker received packed
256-byte rows. The packed input SHA-256 was
`79c9e782e5496df7f82d55a616deb87b783d5709b4fc3561edc11ecc393a110b`;
the verified output SHA-256 was
`0fbba07a833d4dcfc7024eaf313661a0ba8f80a05c6d29b8801c612e10e60dee`.
12,256 bytes changed. Plug-in SHA-256 was
`a1d317c0e18371494bc9c9933684593ca903eb6f3fe262ec06d5147b4c0bcbae`;
Release harness SHA-256 was
`72e534225535ed97f720ed4d425ff2d7ec7a8ca9aea78fb499f1127ec26b9bca`;
guest worker SHA-256 was
`cc3b63d53aa259241d12c0de41d7a7c66c16c07a0f3c8c00bf96352d56878c6b`.
The output hash matches the earlier Blender AEX bake for the same packed input
and frame time. The AEX binary and frame data are not in the repository.

## Host status

| Host | Observed state | Remaining condition |
| --- | --- | --- |
| Blender 4.5.8 LTS | Real AEX baked-image route verified in #1654; this common adapter uses its verified Mac backend | Native live compositor execution and multi-frame session are unverified |
| DaVinci Resolve 21.0.4 | PR #391 Draft verified OFX discovery and control pixels on this Mac | Wire the OFX plug-in to this AEX adapter, then record an AEX frame in Resolve |
| Windows Resolve 18.6.6 | Internal OFX host and local Windows package recorded in #388 | External host discovery and real AEX render remain unverified |

After Effects was not launched for this work.
