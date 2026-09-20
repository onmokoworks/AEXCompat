#[path = "../render_sweep_cli.rs"]
mod render_sweep_cli;

fn main() -> std::process::ExitCode {
    render_sweep_cli::main_entry()
}
