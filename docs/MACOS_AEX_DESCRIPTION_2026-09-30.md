# macOS headless AEX parameter description (#1656)

The macOS harness exposes the same staged setup and canonical `InteractiveParameter` records used by its GUI through a bounded headless command. It describes one AEX and does not render a frame. This supplies parameter metadata for a later Blender control UI; #1654's bake still uses default parameter values.

Build the macOS Release harness and guest worker, then set `AEXCOMPAT_GUEST_WORKER` to the worker binary. With a legally obtained AEX, run:

```sh
broker/target/release/aexcompat-harness --headless --describe-aex "$AEXCOMPAT_PLUGIN_ROOT/DistanceGradation.aex" > description.json
```

Successful stdout is one JSON object matching `contracts/blender/aexcompat_macos_aex_description.schema.json`. It contains schema/version, the staged AEX SHA-256 immediately before and after setup, `files_unchanged`, and paired current/default parameter records. The flag is record-only: if the staged file changes, the description still reports the observed values, while stable file identity cannot be established. If the post-setup staged file cannot be read, its hash and `files_unchanged` are null; this does not turn a successful setup into a failure. If the pre-setup identity read fails, setup still runs, but this evidence-only command exits with `identity_unavailable` after setup because it cannot produce a usable identity record. Raw guest setup output, absolute paths in descriptor labels, local path fields, and arbitrary-data debug summaries are omitted. A descriptor containing an absolute path in a name or choice is rejected without success JSON; ordinary labels such as `Input / Output` are accepted. A missing AEX exits 1 with `plugin_missing`; an unusable worker or unsuccessful setup exits 1 with `description_failed`; malformed command arguments exit 64. Errors write no success JSON to stdout.

On this Apple Silicon Mac, the locally held license-free OLM `DistanceGradation.aex` (SHA-256 `a1d317c0e18371494bc9c9933684593ca903eb6f3fe262ec06d5147b4c0bcbae`) returned 12 parameters in slots 1–12. Examples include `Inside Threshold` (integer, slot 3), `Gradation Color` (color, slot 7), and `Power` (float, slot 10). Two independent descriptions had identical JSON and passed the schema with no absolute paths. The description SHA-256 was `98d00f02cc674dc4f4796a8c3a6ad860b076d26163d802dc6cd203ed3cddec16`; the Release harness SHA-256 was `161b7c78eea1ba384cefef5867ea5d39bf2b9602781e038760d18973d41cd23a` and the Release guest worker SHA-256 was `a12047be981e0bae2924885184dd7f23a340cfe57bee7bdf4fb8bd0fa9421213`. After Effects was not started, and no AEX binary or local raw setup report is committed.

The twelve returned current-value records were also passed unchanged into a Classic render fixture with #1654's 64×64 input and frame time. The resulting packed RGBA8 SHA-256 was `0fbba07a833d4dcfc7024eaf313661a0ba8f80a05c6d29b8801c612e10e60dee`, matching #1654's default bake. This confirms that the described records can be consumed by the existing fixture path for this AEX.

The command reports descriptor defaults from a staged setup. It does not prove that changing a parameter changes rendered pixels, expose Blender controls, or establish After Effects parity. Those are separate follow-up tasks.
