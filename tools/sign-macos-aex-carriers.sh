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
include_native=${AEXCOMPAT_INCLUDE_NATIVE_CARRIER:-0}

case "$include_native" in
  0|1) ;;
  *)
    echo "AEXCOMPAT_INCLUDE_NATIVE_CARRIER must be 0 or 1" >&2
    exit 2
    ;;
esac

for worker in "$arm64_worker"; do
  if [ ! -f "$worker" ]; then
    echo "missing worker: $worker" >&2
    exit 2
  fi
done
if [ "$include_native" = "1" ] && [ ! -f "$native_worker" ]; then
  echo "missing worker: $native_worker" >&2
  exit 2
fi

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
if [ "$include_native" = "1" ]; then
  sign_one "$native_worker" "$root/tools/macos/x86_64-native-carrier.entitlements"
fi
"$root/tools/verify-macos-aex-carriers.sh" "$arm64_worker" "$native_worker"
