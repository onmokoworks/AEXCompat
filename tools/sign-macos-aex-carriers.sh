#!/bin/sh
set -eu

if [ "$(uname -s)" != "Darwin" ]; then
  echo "sign-macos-aex-carriers.sh requires macOS" >&2
  exit 2
fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
identity=${AEXCOMPAT_CODESIGN_IDENTITY:--}
arm64_worker=${1:-"$root/guest/target/release/aex-guest-worker"}
native_worker=${2:-"$root/guest/target/x86_64-apple-darwin/release/aex-guest-worker"}

for worker in "$arm64_worker" "$native_worker"; do
  if [ ! -f "$worker" ]; then
    echo "missing worker: $worker" >&2
    exit 2
  fi
done

sign_one() {
  worker=$1
  entitlements=$2
  if [ "$identity" = "-" ]; then
    codesign --force --sign - --options runtime --entitlements "$entitlements" "$worker"
  else
    codesign --force --sign "$identity" --options runtime --timestamp \
      --entitlements "$entitlements" "$worker"
  fi
}

sign_one "$arm64_worker" "$root/tools/macos/arm64-unicorn.entitlements"
sign_one "$native_worker" "$root/tools/macos/x86_64-native-carrier.entitlements"
"$root/tools/verify-macos-aex-carriers.sh" "$arm64_worker" "$native_worker"
