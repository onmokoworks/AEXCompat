#!/bin/sh
set -eu

if [ "$(uname -s)" != "Darwin" ]; then
  echo "build-macos-aex-carriers.sh requires macOS" >&2
  exit 2
fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
include_native=${AEXCOMPAT_INCLUDE_NATIVE_CARRIER:-0}

case "$include_native" in
  0|1) ;;
  *)
    echo "AEXCOMPAT_INCLUDE_NATIVE_CARRIER must be 0 or 1" >&2
    exit 2
    ;;
esac

cargo build \
  --release \
  --manifest-path "$root/guest/Cargo.toml" \
  -p aex-guest-worker

echo "Unicorn fallback: $root/guest/target/release/aex-guest-worker"
if [ "$include_native" = "1" ]; then
  cargo build \
    --release \
    --target x86_64-apple-darwin \
    --features native-carrier \
    --manifest-path "$root/guest/Cargo.toml" \
    -p aex-guest-worker
  echo "Native carrier:   $root/guest/target/x86_64-apple-darwin/release/aex-guest-worker"
else
  echo "Native carrier:   omitted (set AEXCOMPAT_INCLUDE_NATIVE_CARRIER=1 for trusted-only opt-in)"
fi
echo "Sign and verify:   $root/tools/sign-macos-aex-carriers.sh"
