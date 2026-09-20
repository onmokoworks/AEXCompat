//! Compatibility wrapper for the supported `aexcompat-render-sweep` binary.
//!
//! New automation should invoke the named binary. Keeping this example avoids
//! breaking existing local runbooks while both entry points execute the same
//! canonical implementation.

#[path = "../src/render_sweep_cli.rs"]
mod render_sweep_cli;

fn main() -> std::process::ExitCode {
    render_sweep_cli::main_entry()
}
