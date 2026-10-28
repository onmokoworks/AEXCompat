# Native image geometry

`NativeImageInput::read_raw` / `decode` retain packed little-endian RGBA
samples at the requested AE depth. Their `world_layout` starts as `None`.
Callers may set it to a `render_fixture::FixtureWorldLayout` before calling
`render_native_image_with_dependencies`. This reuses the declarative world's
validation and isolated-worker layout transport; it does not reinterpret
padding bytes as RGBA samples.

The optional layout must describe the same dimensions and pixel format as
the input, with no parameter slot. The positive stride must equal packed
rowbytes plus padding. Padding is at most 256 bytes and a multiple of the
pixel size. Origin components are bounded to ±4096. Extent is a nonempty
rectangle within the world. Dimensions, pixel count, stride arithmetic and
allocation bounds are checked before dispatch, and the worker revalidates
its layout. `padding_byte` initializes the worker allocation; it is not a
claim about another host's unobserved memory.

Primary SmartFX still and temporal checkout worlds retain the primary stride,
padding fill, origin and extent while owning separate pixels. Output samples
and hashes remain logical pixels only. Output layout is managed by the host;
this option configures the primary input world.

PAR, downsample and pre-effect source origin are separate properties in
`HostContext.spatial`. The source origin does not substitute for the PF world
origin. `HostContext.smart_output_request_rect` independently specifies the
SmartFX output demand in layer coordinates; its existing bounds are checked
and Classic requests reject that option. A requested output ROI may extend
outside the input, unlike the input extent. Arbitrary negative input strides,
empty input extents and layout-driven pixel format conversion are not supported.

Verification:

- `cargo test --manifest-path broker/Cargo.toml -p aexcompat-broker --lib native_input_tests`
  exercises each native depth, retained bytes and malformed layouts.
- `aex_worker --kind smart --self-test-parameter-animation` exercises production
  SmartFX still/temporal world preparation, native words, stride, padding,
  origin and extent at all three depths, alongside the animation checks.
- `cargo test --manifest-path broker/Cargo.toml -p aexcompat-broker --test render_session_wrapper native_primary_world_layout -- --nocapture`
  observes independent stride/origin/extent variations in pixels written by
  the Classic layer probe. Requires the worker and SDK layer probe; otherwise
  it reports a skip. These are host behavioral tests, not AE equivalence.

Real AE comparisons must additionally record where each geometry value was
observed. Metadata from adjacent capture effects does not prove object identity
with the target effect's worlds, and JSX PAR readback is not a captured native
rational. Report those limitations and compare every logical sample exactly.
