#[cfg(windows)]
fn main() {
    use aexcompat_broker::selftest::{run, Workers};
    use std::path::{Component, PathBuf};
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 || args[1] != "selftest" || !args[2].ends_with(".json") {
        eprintln!("usage: broker selftest <create-new-json-output>");
        std::process::exit(2);
    }
    let executable = std::env::current_exe().expect("current executable");
    let directory = executable.parent().expect("executable directory");
    let repository = directory
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .expect("repository root");
    let output_root = repository.join("target").join("broker-selftest");
    std::fs::create_dir_all(&output_root).expect("create broker selftest root");
    let requested = PathBuf::from(&args[2]);
    if requested
        .components()
        .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        eprintln!("output traversal is forbidden");
        std::process::exit(2);
    }
    let output = if requested.is_absolute() {
        requested
    } else {
        repository.join(requested)
    };
    let parent = output.parent().expect("output parent");
    std::fs::create_dir_all(parent).expect("create output parent");
    if !parent
        .canonicalize()
        .expect("canonical output parent")
        .starts_with(output_root.canonicalize().expect("canonical output root"))
    {
        eprintln!("output must stay under broker selftest root");
        std::process::exit(2);
    }
    let workers = Workers {
        exit0: directory.join("dummy_exit0.exe"),
        sleep: directory.join("dummy_sleep.exe"),
        abort: directory.join("dummy_abort.exe"),
    };
    let passed = run(&workers, &output).expect("broker selftest");
    std::process::exit(if passed { 0 } else { 1 });
}

#[cfg(not(windows))]
fn main() {
    eprintln!("broker selftest requires Windows");
    std::process::exit(2);
}
