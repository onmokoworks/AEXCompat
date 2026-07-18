# AE Oracle Cross-Machine Runbook

This document defines the minimum contract for reproducing an After Effects
oracle capture on another Windows machine. It applies to real AE renders,
AEX load checks, and CDB/native traces. Ordinary AEXCompat minihost runs do
not require After Effects; this runbook is only for evidence that depends on
the Adobe host.

## What Must Match

The following values are part of the evidence identity. A capture is not
portable merely because the same filename was copied.

| Item | Requirement |
| --- | --- |
| OS | Windows x64; record Windows version and architecture |
| After Effects | Exact major/minor/build, for example `25.2x131` or `26.3` |
| AEX | Exact binary; record full path, byte size, and SHA-256 |
| Renderer | Record `project_gpu_accel_type.current_name` and `.raw`; use Software when the request says so |
| Project | Fresh/empty project policy, bits per channel, working-space and linear-blending settings |
| Input | Exact input file and SHA-256 |
| Output | Format, channel layout, bit depth, compression, color/profile settings and SHA-256 |
| Native tools | CDB version/path, runner revision, and any SDK/build-tool versions |

The AEX hash, AE build, input hash, output settings, and renderer must be
written into the returned manifest. If any required identity value differs,
return `environment_mismatch` or `exact_bind_failure`; do not describe the
result as an AE-exact reference.

## Machine Preparation

Run the following in PowerShell on the capture machine and keep the output in
the run directory:

```powershell
$env:PROCESSOR_ARCHITECTURE
Get-ComputerInfo | Select-Object WindowsProductName,WindowsVersion,OsBuildNumber,OsArchitecture
Get-Command cdb.exe -ErrorAction SilentlyContinue | Select-Object Source,Version
& cargo --version
& python --version
```

Install or make available:

- Windows 10/11 x64.
- The exact requested After Effects build.
- Windows SDK debugging tools containing `cdb.exe`.
- Rust/Cargo, Python, and Visual Studio C++ Build Tools when the runner builds native helpers.
- The local After Effects SDK when compiling SDK-backed probes or workers.

Set the SDK root only when a build or probe needs it, then open a new
PowerShell session:

```powershell
[Environment]::SetEnvironmentVariable(
  'AFTER_EFFECTS_SDK_ROOT',
  'C:\path\to\AfterEffectsSDK',
  'User'
)
```

## AEX Installation and Load Gate

Do not copy an AEX by name alone. Resolve the requested binary, hash it before
launch, and record the result:

```powershell
$aex = 'C:\path\to\OLMBlur.aex'
Get-Item $aex | Select-Object FullName,Length,LastWriteTimeUtc
Get-FileHash $aex -Algorithm SHA256
```

Install the AEX in one location visible to the requested AE build. Do not
leave a second copy with the same effect identity in a common MediaCore folder
and an AE-version-specific folder. AE may show a duplicate-plugin warning or
load a different copy than the runner expects.

Before capturing:

1. Close every AfterFX process.
2. Search the AE-specific and common plug-in directories for duplicate copies.
3. Confirm the selected path and SHA-256 in the runner configuration.
4. Start AE with a fresh project and verify that the effect appears in the Effect menu.
5. Confirm that the runner's loaded-module gate observes the same path/hash.

Example duplicate check:

```powershell
Get-ChildItem 'C:\Program Files\Adobe', $env:APPDATA -Recurse -File \
  -Filter 'OLMBlur.aex' -ErrorAction SilentlyContinue |
  ForEach-Object {
    [pscustomobject]@{
      Path = $_.FullName
      Size = $_.Length
      SHA256 = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
  }
```

The path above is illustrative. Use the actual effect filename and the
installation paths accepted by the target AE build. A file existing on disk
is not proof that AE loaded it.

## Runner Contract

Every AE-dependent runner should accept or document these values:

```text
PackageRoot
WorkRoot
AexPath
AfterFxPath
CdbPath
```

The runner should fail before rendering when a required file is missing, the
AEX hash is wrong, or AE is the wrong build. It should use a fresh AE process
per isolated case when the request requires fresh-process evidence.

The runner should retain, at minimum:

- The exact runner script or immutable runner revision.
- `status.json` and a machine/environment manifest.
- AE JSX/script logs and AfterFX stdout/stderr.
- CDB command script, stdout/stderr, and retained trace.
- Input and output files plus their SHA-256 hashes.
- Loaded module path, module base, process ID, AEX hash, AE version, renderer and BPC.
- Any AE warning, crash, modal-dialog, timeout, or missing-module evidence.

## Example Capture Invocation

Use package-specific defaults when available. Otherwise invoke the runner with
explicit paths rather than relying on the current machine's defaults:

```powershell
$package = 'C:\AEXCompat\capture-package'
$work = Join-Path $package 'work'
$aex = 'C:\Program Files\Adobe\Common\Plug-ins\7.0\MediaCore\OLM\OLMBlur.aex'
$ae = 'C:\Program Files\Adobe\Adobe After Effects 2026\Support Files\AfterFX.exe'
$cdb = 'C:\Program Files (x86)\Windows Kits\10\Debuggers\x64\cdb.exe'

& powershell.exe -NoProfile -ExecutionPolicy Bypass -File `
  (Join-Path $package 'run.ps1') `
  -PackageRoot $package -WorkRoot $work `
  -AexPath $aex -AfterFxPath $ae -CdbPath $cdb
```

Do not run a second AE runner while the first AE process, modal dialog, or CDB
session is still alive. If AE crashes, retain the crash/failure artifact,
close stale processes, and start a new isolated run rather than reusing a
partially written result directory.

## Acceptance and Return

Use explicit statuses:

- `answered`: every required case and evidence field is present and identity checks pass.
- `environment_mismatch`: the machine, AE build, AEX hash, renderer, BPC or output contract differs.
- `exact_bind_failure`: the requested AEX or runtime callback could not be proven in the launched AE process.
- `failed_partial`: some artifacts exist, but required cases or files are missing.

For AE exactness, use the request's rule only. In particular, a threshold pass,
off-by-one result, or guarded/approximate observation is not an exact pass.
For a liveness or diagnostic request, set `exactness_claim=false` and record
the observation without converting it into a pixel-equivalence claim.

Return a zip containing the manifest, logs, traces and required images/EXRs.
Verify the zip after creation:

```powershell
$zip = 'C:\AEXCompat\returns\RETURN_<request>.zip'
Expand-Archive $zip -DestinationPath (Join-Path $env:TEMP 'aexcompat-return-check') -Force
Test-Path (Join-Path $env:TEMP 'aexcompat-return-check\status.json')
```

## What Transfers and What Does Not

The repository, runner scripts, JSX, request manifests and analysis reports
transfer through Git. AE itself, proprietary AEX binaries, SDK installations,
GPU drivers and local plug-in registrations do not. Transfer their verified
metadata and obtain/install the binary through an authorized channel on the
target machine; do not commit proprietary AEX files or secrets.

The target machine can reproduce the capture only after the preflight manifest
passes. If it cannot install the same AE build or cannot prove the same AEX
was loaded, it can still run static/minihost analysis, but its result must be
marked as non-AE-oracle evidence.
