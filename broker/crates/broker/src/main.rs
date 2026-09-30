#[cfg(windows)]
fn main() {
    use aexcompat_broker::selftest::{Workers, run};
    use std::path::{Component, PathBuf};
    aexcompat_broker::observability::init();
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: broker <operation> <json arguments>");
        std::process::exit(2);
    }
    let operation = aexcompat_broker::cli_operation::canonical_operation(&args[1]);
    let executable = std::env::current_exe().expect("current executable");
    let directory = executable.parent().expect("executable directory");
    let repository = directory
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .expect("repository root");
    if args.len() == 4 && args[1] == "validate-render-request" {
        let accepted = match aexcompat_broker::render_request::run(
            repository,
            &PathBuf::from(&args[2]),
            &PathBuf::from(&args[3]),
        ) {
            Ok(accepted) => accepted,
            Err(_) => {
                eprintln!("render request validation failed");
                std::process::exit(2);
            }
        };
        std::process::exit(if accepted { 0 } else { 3 });
    }
    if args.len() == 4 && args[1] == "render-video-batch" {
        let passed = match aexcompat_broker::render_session::run_video_batch(
            repository,
            &PathBuf::from(&args[2]),
            &PathBuf::from(&args[3]),
            // The CLI inherits the broker's environment and reads the opt-in
            // minidump directory from AEXCOMPAT_MINIDUMP_DIR as before.
            &Default::default(),
        ) {
            Ok(passed) => passed,
            Err(_) => {
                eprintln!("batch video render session failed");
                std::process::exit(2);
            }
        };
        std::process::exit(if passed { 0 } else { 3 });
    }
    if args.len() == 4 && operation == "classic-parameter-request" {
        let passed = match aexcompat_broker::render_request::execute(
            repository,
            &PathBuf::from(&args[2]),
            &PathBuf::from(&args[3]),
        ) {
            Ok(passed) => passed,
            Err(_) => {
                eprintln!("parameterized render request failed");
                std::process::exit(2);
            }
        };
        std::process::exit(if passed { 0 } else { 3 });
    }
    if args.len() == 4 && operation == "smart-parameter-request" {
        let passed = match aexcompat_broker::render_request::execute_smart(
            repository,
            &PathBuf::from(&args[2]),
            &PathBuf::from(&args[3]),
        ) {
            Ok(passed) => passed,
            Err(_) => {
                eprintln!("parameterized SmartFX request failed");
                std::process::exit(2);
            }
        };
        std::process::exit(if passed { 0 } else { 3 });
    }
    if args.len() == 5 && operation == "smart-suite-fault" {
        let passed = match aexcompat_broker::render_request::execute_smart_suite_fault(
            repository,
            &args[2],
            &args[3],
            &PathBuf::from(&args[4]),
        ) {
            Ok(passed) => passed,
            Err(_) => {
                eprintln!("SmartFX suite fault conformance failed");
                std::process::exit(2);
            }
        };
        std::process::exit(if passed { 0 } else { 3 });
    }
    if args.len() == 5 && operation == "smart-mask-scene" {
        let passed = match aexcompat_broker::render_request::execute_smart_mask_scene(
            repository,
            &args[2],
            &args[3],
            &PathBuf::from(&args[4]),
        ) {
            Ok(passed) => passed,
            Err(_) => {
                eprintln!("SmartFX mask scene conformance failed");
                std::process::exit(2);
            }
        };
        std::process::exit(if passed { 0 } else { 3 });
    }
    if args.len() == 4 && operation == "discovery" {
        let output = PathBuf::from(&args[3]);
        if !args[3].ends_with(".json") {
            eprintln!("observation output must be JSON");
            std::process::exit(2);
        }
        let worker = repository.join("target/minihost-build/aex_worker.exe");
        let result = aexcompat_broker::l2::run(repository, &worker, &args[2], &output);
        let passed = match result {
            Ok(passed) => passed,
            Err(_) => {
                eprintln!("registered observation run failed");
                std::process::exit(2);
            }
        };
        std::process::exit(if passed { 0 } else { 1 });
    }
    if args.len() != 3 || !args[2].ends_with(".json") {
        eprintln!(
            "usage: broker <selftest|discovery-scattermap|classic-scattermap|smart-scattermap> <create-new-json-output>"
        );
        std::process::exit(2);
    }
    if operation == "discovery-scattermap" {
        let worker = repository.join("target/minihost-build/aex_worker.exe");
        let passed =
            aexcompat_broker::l2::run(repository, &worker, "scattermap", &PathBuf::from(&args[2]))
                .expect("discovery broker run");
        std::process::exit(if passed { 0 } else { 1 });
    }
    if operation == "classic-scattermap" {
        let worker = repository.join("target/minihost-build/aex_worker.exe");
        let passed = aexcompat_broker::render::run(
            repository,
            &worker,
            "scattermap",
            "default",
            &PathBuf::from(&args[2]),
        )
        .expect("classic broker run");
        std::process::exit(if passed { 0 } else { 1 });
    }
    if operation == "smart-scattermap" {
        let worker = repository.join("target/minihost-build/aex_worker.exe");
        let passed = aexcompat_broker::smart::run(
            repository,
            &worker,
            "scattermap",
            "default",
            &PathBuf::from(&args[2]),
        )
        .expect("SmartFX broker run");
        std::process::exit(if passed { 0 } else { 1 });
    }
    let smart_case = match operation {
        "smart-identity-scattermap" => Some("identity"),
        "smart-horizontal-scattermap" => Some("horizontal"),
        "smart-vertical-no-repeat-scattermap" => Some("vertical_no_repeat"),
        "smart-mixed-scattermap" => Some("mixed"),
        "smart-amount-max-scattermap" => Some("amount_max"),
        "smart-seed-max-scattermap" => Some("seed_max"),
        "smart-mix-zero-scattermap" => Some("mix_zero"),
        "smart-odd-dimensions-scattermap" => Some("odd_dimensions"),
        "smart-padded-stride-scattermap" => Some("padded_stride"),
        "smart-connected-map-scattermap" => Some("connected_map"),
        "smart-inverted-map-scattermap" => Some("inverted_map"),
        "smart-deep16-scattermap" => Some("deep16_default"),
        "smart-float32-scattermap" => Some("float32_default"),
        "smart-gpu-negotiation-scattermap" => Some("gpu_fallback_float32"),
        "smart-error-missing-input-scattermap" => Some("error_missing_input"),
        "smart-crash-null-output-scattermap" => Some("crash_null_output_world"),
        "smart-temporal-context-scattermap" => Some("temporal_context"),
        "smart-partial-output-request-scattermap" => Some("partial_output_request"),
        _ => None,
    };
    if let Some(case_id) = smart_case {
        let worker = repository.join("target/minihost-build/aex_worker.exe");
        let passed = aexcompat_broker::smart::run(
            repository,
            &worker,
            "scattermap",
            case_id,
            &PathBuf::from(&args[2]),
        )
        .expect("extended SmartFX broker run");
        std::process::exit(if passed { 0 } else { 1 });
    }
    let extended_case = match operation {
        "classic-identity-scattermap" => Some("identity"),
        "classic-horizontal-scattermap" => Some("horizontal"),
        "classic-vertical-no-repeat-scattermap" => Some("vertical_no_repeat"),
        "classic-mixed-scattermap" => Some("mixed"),
        "classic-amount-max-scattermap" => Some("amount_max"),
        "classic-seed-max-scattermap" => Some("seed_max"),
        "classic-mix-zero-scattermap" => Some("mix_zero"),
        "classic-odd-dimensions-scattermap" => Some("odd_dimensions"),
        "classic-padded-stride-scattermap" => Some("padded_stride"),
        "classic-connected-map-scattermap" => Some("connected_map"),
        "classic-inverted-map-scattermap" => Some("inverted_map"),
        "classic-partial-extent-hint-scattermap" => Some("partial_extent_hint"),
        "classic-threaded-default-scattermap" => Some("threaded_default"),
        _ => None,
    };
    if let Some(case_id) = extended_case {
        let worker = repository.join("target/minihost-build/aex_worker.exe");
        let passed = aexcompat_broker::render::run(
            repository,
            &worker,
            "scattermap",
            case_id,
            &PathBuf::from(&args[2]),
        )
        .expect("extended classic broker run");
        std::process::exit(if passed { 0 } else { 1 });
    }
    if operation != "selftest" {
        eprintln!("unknown broker operation");
        std::process::exit(2);
    }
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
