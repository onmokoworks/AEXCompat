// Keep the public image-rendering surface in one module while grouping the
// implementation into bounded, reviewable source files. `include!` preserves
// the original item scope and therefore does not change visibility or paths.
include!("image_render/diagnostics.rs");
include!("image_render/types_and_transport.rs");
include!("image_render/render_operations.rs");
include!("image_render/inspection_and_probes.rs");
include!("image_render/session.rs");
include!("image_render/tests.rs");
