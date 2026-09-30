// Keep these implementation slices in one Rust module so the mechanical split
// does not change visibility, cfg evaluation, or the harness's public surface.
include!("windows/ui.rs");
include!("windows/preflight.rs");
include!("windows/render_contract.rs");
include!("windows/live_session.rs");
include!("windows/app.rs");
include!("windows/cli.rs");
include!("windows/tests.rs");
