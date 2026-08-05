#!/bin/sh
set -eu

if [ "$(uname -s)" != "Darwin" ]; then
  echo "prepare-local-macos-aex-carriers.sh requires macOS" >&2
  exit 2
fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
output=${1:-"$root/guest/target/aexcompat-macos-carriers-local.dmg"}
arm64_worker="$root/guest/target/release/aex-guest-worker"
native_worker="$root/guest/target/x86_64-apple-darwin/release/aex-guest-worker"
smoke_aex=${AEXCOMPAT_SMOKE_AEX:-}
smoke_input_png=${AEXCOMPAT_SMOKE_INPUT_PNG:-}

if { [ -n "$smoke_aex" ] && [ -z "$smoke_input_png" ]; } || \
   { [ -z "$smoke_aex" ] && [ -n "$smoke_input_png" ]; }; then
  echo "AEXCOMPAT_SMOKE_AEX and AEXCOMPAT_SMOKE_INPUT_PNG must be set together" >&2
  exit 2
fi
if [ -n "$smoke_aex" ] && { [ ! -f "$smoke_aex" ] || [ ! -f "$smoke_input_png" ]; }; then
  echo "AEXCOMPAT_SMOKE_AEX or AEXCOMPAT_SMOKE_INPUT_PNG does not exist" >&2
  exit 2
fi

"$root/tools/build-macos-aex-carriers.sh"
AEXCOMPAT_CODESIGN_IDENTITY=- "$root/tools/sign-macos-aex-carriers.sh" \
  "$arm64_worker" "$native_worker"
AEXCOMPAT_DISTRIBUTION_TIER=local-adhoc \
  "$root/tools/package-macos-aex-carriers.sh" \
  "$arm64_worker" "$native_worker" "$output"
if [ -n "$smoke_aex" ]; then
  "$root/tools/verify-macos-aex-carrier-package.sh" \
    "$output" "$smoke_aex" "$smoke_input_png"
else
  "$root/tools/verify-macos-aex-carrier-package.sh" "$output"
fi

echo "Local macOS carrier distribution is ready: $output"
