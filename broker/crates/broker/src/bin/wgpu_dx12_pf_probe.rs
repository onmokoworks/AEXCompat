use aexcompat_broker::wgpu_dx12_pf_probe::{run_from_manifest, write_report_create_new};
use std::path::PathBuf;

fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        eprintln!(
            "usage: wgpu-dx12-pf-probe <repository> <artifacts.json> <create-new-output.json>"
        );
        std::process::exit(2);
    }
    let repository = PathBuf::from(&args[0]);
    let manifest = PathBuf::from(&args[1]);
    let output = PathBuf::from(&args[2]);
    let result = run_from_manifest(&repository, &manifest).and_then(|report| {
        write_report_create_new(&repository, &output, &report)?;
        if report.passed {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "wgpu DX12 PF probe completed without readiness",
            ))
        }
    });
    if let Err(error) = result {
        eprintln!("wgpu DX12 PF probe failed: {error}");
        std::process::exit(1);
    }
}
