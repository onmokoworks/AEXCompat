#!/bin/sh
set -eu

if [ "$(uname -s)" != "Darwin" ]; then
  echo "verify-macos-aex-carriers.sh requires macOS" >&2
  exit 2
fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
arm64_worker=${1:-"$root/guest/target/release/aex-guest-worker"}
native_worker=${2:-"$root/guest/target/x86_64-apple-darwin/release/aex-guest-worker"}
scratch=$(mktemp -d "${TMPDIR:-/tmp}/aexcompat-signing.XXXXXX")
trap 'rm -rf "$scratch"' EXIT HUP INT TERM

verify_one() {
  worker=$1
  architecture=$2
  required_key=$3
  plist=$4

  file "$worker" | grep -q "$architecture"
  codesign --verify --strict --verbose=2 "$worker"
  codesign -d --entitlements :- "$worker" >"$plist" 2>/dev/null
  plutil -lint "$plist" >/dev/null
  [ "$(plutil -extract "$required_key" raw -o - "$plist")" = "true" ]
  codesign -dvv "$worker" 2>&1 | grep -q 'flags=.*runtime'

  for forbidden in com.apple.security.get-task-allow \
    com.apple.security.cs.disable-library-validation \
    com.apple.security.cs.disable-executable-page-protection \
    com.apple.security.cs.allow-dyld-environment-variables; do
    if plutil -extract "$forbidden" raw -o - "$plist" >/dev/null 2>&1; then
      echo "forbidden entitlement on $worker: $forbidden" >&2
      exit 1
    fi
  done
}

verify_one "$arm64_worker" arm64 com.apple.security.cs.allow-jit "$scratch/arm64.plist"
verify_one "$native_worker" x86_64 com.apple.security.cs.allow-unsigned-executable-memory \
  "$scratch/native.plist"

"$arm64_worker" --help >/dev/null 2>&1 || [ "$?" -eq 2 ]
arch -x86_64 "$native_worker" --help >/dev/null 2>&1 || [ "$?" -eq 2 ]

echo "macOS carrier signatures, Hardened Runtime flags, architectures, and launch probes verified"
