# AEXCompat

**A compatibility host for loading, rendering, and debugging `.aex` plug-ins outside After Effects.**

[日本語](README.md) · [Quick start](#quick-start) · [Documentation](#documentation) · [License](#license)

<p align="center">
  <img src="docs/images/aexcompat-gui.png" alt="AEXCompat GUI with analysis logs, Effect Controls, and input/output viewers" width="1200">
</p>

The Rust/egui GUI being unified across Windows and Apple Silicon combines AEX selection, Effect Controls, input/output comparison, and diagnostic logs in one workspace.

## What is AEXCompat?

AEXCompat is a clean-room compatibility host that feeds images or audio to Windows x64 `.aex` plug-ins and displays, saves, and diagnoses their output.

- Rust desktop harness and C++/MSVC worker on Windows
- arm64 Unicorn worker for Windows x64 guest execution on Apple Silicon
- Effect Controls, Classic Render, SmartFX, and ARGB8/16/32F
- Structured selector, Suite, parameter, crash, and hang diagnostics
- Pixel comparison with reference output captured from After Effects

It is not intended to recreate the complete After Effects application, AEP editor, or full AEGP host.

| Platform | Execution route |
|---|---|
| Windows x64 | Desktop harness + native C++/MSVC worker |
| Apple Silicon | arm64 Unicorn worker executing Windows x64 guest code |

## Quick start

### Apple Silicon Mac

With Rust/Cargo installed, one command builds the arm64 Release worker, applies an ad-hoc signature, creates a DMG, mounts it, and verifies the package. Windows, Rosetta, and an Adobe certificate are not required.

```sh
git clone https://github.com/onmokoworks/AEXCompat.git
cd AEXCompat
tools/prepare-local-macos-aex-carriers.sh /tmp/aexcompat-local.dmg
```

To render a local AEX and PNG:

```sh
AEXCOMPAT_SMOKE_AEX=/absolute/path/to/effect.aex \
AEXCOMPAT_SMOKE_INPUT_PNG=/absolute/path/to/input.png \
  tools/prepare-local-macos-aex-carriers.sh /tmp/aexcompat-local-smoke.dmg
```

The AEX and input image are not included in the DMG. The normal path uses only the arm64 Unicorn worker. The optional Rosetta carrier is included only with `AEXCOMPAT_INCLUDE_NATIVE_CARRIER=1` and is intended for trusted AEX plug-ins.

### Windows

Requirements:

- Windows 10/11 x64
- Rust/Cargo
- Visual Studio or Build Tools with MSVC C++ and the Windows SDK
- CMake

Launch the GUI:

```powershell
git clone https://github.com/onmokoworks/AEXCompat.git
cd AEXCompat\broker
cargo run -p aexcompat-harness
```

Inspecting or rendering an AEX also requires the `minihost/` workers. The Adobe SDK is needed only when building probes or SDK fixtures. See [Build Requirements](docs/BUILD_REQUIREMENTS.md) for exact commands and toolsets.

## Workflow

1. Select an AEX.
2. A background `PF_PARAMS_SETUP` run builds the Effect Controls.
3. Select an input image and edit parameters.
4. Run **Render current frame** or **Render and save PNG**.
5. Compare the input and AEX output in the viewer.

Supported image inputs are PNG, JPEG, BMP, TIFF, and WebP; output is PNG. The host also models multiple Layer inputs, time/FPS, downsampling, pixel aspect, sequence state, and mono float32 audio.

## Architecture

```text
Desktop Harness / CLI
        ↓
Rust Broker (validation, staging, provenance)
        ↓
Isolated Worker Process
        ↓
Clean-room Effect Host (selectors, PF worlds, Suites)
        ↓
AEX → image / audio / diagnostics
```

| Directory | Purpose |
|---|---|
| `broker/` | Rust broker, desktop harness, and isolated launch |
| `minihost/` | C++ worker providing the AE Effect ABI and PF Suites |
| `guest/` | Guest worker for Windows x64 AEX execution on Apple Silicon |
| `instruments/` | Probe AEX projects for SDK ABI, Suite, and selector behavior |
| `tests/` | Contract, boundary, regression, and fail-closed tests |
| `analysis/` | AE oracle and runtime evidence |

## Tests

```powershell
uv sync --locked
cargo test --manifest-path broker\Cargo.toml --workspace
uv run python -m pytest -q
```

SDK, checkout-built artifact, and machine-bound evidence tests are explicit opt-ins. Public-repository and fork CI runs validate the source-only scope and do not fetch or redistribute the Adobe SDK. See [Build Requirements](docs/BUILD_REQUIREMENTS.md) for the complete matrix.

## Current limitations

- Missing Suites and effect-specific host assumptions can stop loading or rendering
- Worker completion, image production, and AE pixel equivalence are separate milestones
- Custom UI, GPU backends, pixel depths, multiple inputs, and sequence semantics vary by plug-in

## Documentation

| Topic | Document |
|---|---|
| Build, SDK, and CI requirements | [Build Requirements](docs/BUILD_REQUIREMENTS.md) |
| Current compatibility | [Compatibility Status](docs/COMPATIBILITY_STATUS_2026-07-16.md) |
| Direction and roadmap | [Project Direction](docs/PROJECT_DIRECTION.md) |
| Mac AEX validation method and dated results | [macOS Unicorn Corpus Metrics](docs/MACOS_UNICORN_CORPUS_METRICS.md) |
| AEX porting and analysis | [AEX Porting Dossier](docs/aex-porting-dossier.md) |
| Windows native hardening | [Windows Native Hardening Plan](docs/WINDOWS_NATIVE_HARDENING_PLAN_2026-07-16.md) |
| Cross-machine AE oracle capture | [AE Oracle Cross-Machine Runbook](docs/AE_ORACLE_CROSS_MACHINE_RUNBOOK_2026-07-18.md) |
| Publication and third-party boundary | [Public Release Audit](docs/PUBLIC_RELEASE_AUDIT.md) |
| Vulnerability reporting | [Security Policy](SECURITY.md) |
| Development and submission rules | [Contributing](CONTRIBUTING.md) |

The [documentation index](docs/README.md) lists further design and validation documents.

## Contributing

Compatibility work starts with observed behavior from a real AEX, SDK sample, or self-authored probe. Changes should model the smallest general host capability rather than a fixture-specific branch, add focused boundary tests, and compare against an After Effects oracle where possible.

Do not post proprietary AEX plug-ins, Adobe SDK content, DLLs, dumps, private assets/corpora, secrets, or personal paths in issues or pull requests.

## License

AEXCompat-authored material that contributors have the right to license is available under the [Mozilla Public License 2.0](LICENSE). Third-party material, the Adobe SDK, and private AEX plug-ins retain their own terms. See the [Public Release Audit](docs/PUBLIC_RELEASE_AUDIT.md) for Unicorn-linked binary distribution, dependency notices, and publication requirements.

Adobe, After Effects, and related product names are trademarks of their respective owners. AEXCompat is not an official Adobe project.
