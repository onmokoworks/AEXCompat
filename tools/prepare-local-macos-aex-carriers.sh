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

"$root/tools/build-macos-aex-carriers.sh"
AEXCOMPAT_CODESIGN_IDENTITY=- "$root/tools/sign-macos-aex-carriers.sh" \
  "$arm64_worker" "$native_worker"
AEXCOMPAT_DISTRIBUTION_TIER=local-adhoc \
  "$root/tools/package-macos-aex-carriers.sh" \
  "$arm64_worker" "$native_worker" "$output"
"$root/tools/verify-macos-aex-carrier-package.sh" "$output"

echo "Local macOS carrier distribution is ready: $output"
