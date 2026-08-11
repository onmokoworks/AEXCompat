// Keep the public image-rendering surface in one module while grouping the
// implementation into bounded, reviewable source files. `include!` preserves
// the original item scope and therefore does not change visibility or paths.
pub use crate::parameter_animation::{
    AnimationInterpolation, AnimationTime, AnimationValue, ParameterAnimation,
    ParameterAnimationKey, parameter_animation_sidecar_json,
};

pub use crate::render_artifacts::{
    RenderArtifactConditions, RenderArtifactKind, write_float32_exr_artifact,
    write_raw_world_artifact, write_raw_world_checkpoint_artifact,
};
pub use crate::render_fixture::InteractiveParameter;
pub use crate::render_pixel_format::RenderPixelFormat;

include!("image_render/diagnostics.rs");
include!("image_render/types_and_transport.rs");
include!("image_render/render_operations.rs");
include!("image_render/declarative_fixture.rs");
include!("image_render/inspection_and_probes.rs");
include!("image_render/session.rs");
include!("image_render/tests.rs");
