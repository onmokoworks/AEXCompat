#!/bin/sh
set -eu

if [ "$(uname -s)" != "Darwin" ]; then
  echo "verify-macos-aex-carrier-package.sh requires macOS" >&2
  exit 2
fi

artifact=${1:-}
if [ -z "$artifact" ] || [ ! -f "$artifact" ]; then
  echo "usage: $0 <aexcompat-macos-carriers.dmg> [x64.aex input.png]" >&2
  exit 2
fi
smoke_aex=${2:-}
smoke_input_png=${3:-}
if { [ -n "$smoke_aex" ] && [ -z "$smoke_input_png" ]; } || \
   { [ -z "$smoke_aex" ] && [ -n "$smoke_input_png" ]; }; then
  echo "smoke validation requires both x64.aex and input.png" >&2
  exit 2
fi
if [ -n "$smoke_aex" ] && { [ ! -f "$smoke_aex" ] || [ ! -f "$smoke_input_png" ]; }; then
  echo "smoke AEX or input PNG does not exist" >&2
  exit 2
fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/aexcompat-package-verify.XXXXXX")
mount_point="$scratch/mount"
attached=0

detach_image() {
  attempts=0
  while [ "$attempts" -lt 5 ]; do
    if hdiutil detach "$mount_point" >/dev/null 2>&1; then
      attached=0
      return 0
    fi
    attempts=$((attempts + 1))
    sleep 0.2
  done
  if hdiutil detach -force "$mount_point" >/dev/null 2>&1; then
    attached=0
    return 0
  fi
  echo "failed to detach owned package mount: $mount_point" >&2
  return 1
}

cleanup() {
  if [ "$attached" = "1" ]; then
    detach_image || true
  fi
  if [ "$attached" = "0" ]; then
    rm -rf "$scratch"
  fi
}
trap cleanup EXIT HUP INT TERM

mkdir -p "$mount_point"
hdiutil verify "$artifact" >/dev/null
hdiutil attach -nobrowse -readonly -mountpoint "$mount_point" "$artifact" >/dev/null
attached=1

/usr/bin/python3 - "$mount_point" <<'PY'
import hashlib
import json
from pathlib import Path
import sys

root = Path(sys.argv[1])
manifest_path = root / "manifest.json"
data = json.loads(manifest_path.read_text(encoding="utf-8"))
if set(data) != {"schema", "distribution_tier", "workers"}:
    raise SystemExit("manifest has unexpected or missing top-level keys")
if data["schema"] != "aexcompat-macos-carriers-v1":
    raise SystemExit("unsupported package manifest schema")
if data["distribution_tier"] not in {"local-adhoc", "developer-id"}:
    raise SystemExit("unsupported package distribution tier")

workers = data["workers"]
if not isinstance(workers, list) or not workers or len(workers) > 2:
    raise SystemExit("package must contain one or two workers")
expected = {
    "arm64": ("unicorn", "arm64/aex-guest-worker"),
    "x86_64": ("native-carrier-trusted-only", "x86_64/aex-guest-worker"),
}
architectures = []
declared_paths = {"manifest.json"}
for worker in workers:
    if set(worker) != {"architecture", "backend", "path", "sha256", "size"}:
        raise SystemExit("worker manifest entry has unexpected or missing keys")
    architecture = worker["architecture"]
    if architecture in architectures or architecture not in expected:
        raise SystemExit("worker architecture is duplicate or unsupported")
    architectures.append(architecture)
    backend, relative = expected[architecture]
    if worker["backend"] != backend or worker["path"] != relative:
        raise SystemExit("worker backend or path does not match its architecture")
    path = root / relative
    payload = path.read_bytes()
    if worker["size"] != len(payload):
        raise SystemExit("worker size does not match manifest")
    if worker["sha256"] != hashlib.sha256(payload).hexdigest():
        raise SystemExit("worker SHA-256 does not match manifest")
    declared_paths.add(relative)

if architectures[0] != "arm64":
    raise SystemExit("arm64 Unicorn worker must be the first and mandatory worker")
actual_paths = {
    path.relative_to(root).as_posix()
    for path in root.rglob("*")
    if path.is_file()
}
if actual_paths != declared_paths:
    raise SystemExit("package contains undeclared or missing files")
PY

arm64_worker="$mount_point/arm64/aex-guest-worker"
file "$arm64_worker" | grep -q arm64
codesign --verify --strict --verbose=2 "$arm64_worker"
"$arm64_worker" --help >/dev/null 2>&1 || [ "$?" -eq 2 ]

if [ -n "$smoke_aex" ]; then
  smoke_output="$scratch/smoke-output.png"
  smoke_report="$scratch/smoke-diagnostic.json"
  smoke_stderr="$scratch/smoke-stderr.txt"
  if ! "$arm64_worker" render-trace-png \
      "$smoke_aex" "$smoke_input_png" "$smoke_output" \
      >"$smoke_report" 2>"$smoke_stderr"; then
    echo "packaged arm64 render/diagnose smoke failed" >&2
    sed -n '1,80p' "$smoke_stderr" >&2
    exit 1
  fi
  /usr/bin/python3 "$root/tools/verify_macos_aex_smoke_report.py" \
    "$smoke_report" "$smoke_output"
fi

native_worker="$mount_point/x86_64/aex-guest-worker"
if [ -f "$native_worker" ]; then
  file "$native_worker" | grep -q x86_64
  codesign --verify --strict --verbose=2 "$native_worker"
  arch -x86_64 "$native_worker" --help >/dev/null 2>&1 || [ "$?" -eq 2 ]
fi

detach_image
if [ -n "$smoke_aex" ]; then
  echo "macOS carrier DMG integrity, arm64 Unicorn render, diagnostics, and cleanup verified"
else
  echo "macOS carrier DMG integrity, manifest, signatures, architectures, and launch probes verified"
fi
