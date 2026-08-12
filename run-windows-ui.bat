@echo off
setlocal
pushd "%~dp0" || exit /b 1

echo Building AEXCompat Windows UI (Release)...
cargo build --manifest-path broker\Cargo.toml -p aexcompat-harness --release
if errorlevel 1 (
    echo Build failed.
    popd
    exit /b 1
)

echo Starting AEXCompat Windows UI...
start "AEXCompat Windows UI" "%CD%\broker\target\release\aexcompat-harness.exe"
popd
