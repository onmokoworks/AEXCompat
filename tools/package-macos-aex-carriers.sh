#!/bin/sh
set -eu

if [ "$(uname -s)" != "Darwin" ]; then
  echo "package-macos-aex-carriers.sh requires macOS" >&2
  exit 2
fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
arm64_worker=${1:-"$root/guest/target/release/aex-guest-worker"}
native_worker=${2:-"$root/guest/target/x86_64-apple-darwin/release/aex-guest-worker"}
output=${3:-"$root/guest/target/aexcompat-macos-carriers.dmg"}
distribution_tier=${AEXCOMPAT_DISTRIBUTION_TIER:-local-adhoc}
include_native=${AEXCOMPAT_INCLUDE_NATIVE_CARRIER:-0}
scratch=$(mktemp -d "${TMPDIR:-/tmp}/aexcompat-package.XXXXXX")
trap 'rm -rf "$scratch"' EXIT HUP INT TERM

case "$distribution_tier" in
  local-adhoc|developer-id) ;;
  *)
    echo "unsupported AEXCOMPAT_DISTRIBUTION_TIER: $distribution_tier" >&2
    exit 2
    ;;
esac
case "$include_native" in
  0|1) ;;
  *)
    echo "AEXCOMPAT_INCLUDE_NATIVE_CARRIER must be 0 or 1" >&2
    exit 2
    ;;
esac

if [ -e "$output" ]; then
  echo "refusing to replace existing package: $output" >&2
  exit 2
fi

verify_worker() {
  worker=$1
  architecture=$2

  if [ ! -f "$worker" ]; then
    echo "missing worker: $worker" >&2
    exit 2
  fi
  file "$worker" | grep -q "$architecture"
  codesign --verify --strict --verbose=2 "$worker"
  signature=$(codesign -dvv "$worker" 2>&1)
  echo "$signature" | grep -q 'flags=.*runtime'
  if [ "$distribution_tier" = "local-adhoc" ]; then
    echo "$signature" | grep -q '^Signature=adhoc$' || {
      echo "local-adhoc package requires an ad-hoc signed worker: $worker" >&2
      exit 2
    }
  else
    echo "$signature" | grep -q '^Authority=Developer ID Application:' || {
      echo "worker is not signed with Developer ID Application: $worker" >&2
      exit 2
    }
  fi
}

verify_worker "$arm64_worker" arm64
if [ "$include_native" = "1" ]; then
  verify_worker "$native_worker" x86_64
fi

payload="$scratch/AEXCompat Carriers"
mkdir -p "$payload/arm64"
ditto "$arm64_worker" "$payload/arm64/aex-guest-worker"
if [ "$include_native" = "1" ]; then
  mkdir -p "$payload/x86_64"
  ditto "$native_worker" "$payload/x86_64/aex-guest-worker"
fi

arm64_sha=$(shasum -a 256 "$payload/arm64/aex-guest-worker" | awk '{print $1}')
arm64_size=$(stat -f %z "$payload/arm64/aex-guest-worker")

{
  printf '%s\n' '{'
  printf '  "schema": "aexcompat-macos-carriers-v1",\n'
  printf '  "distribution_tier": "%s",\n' "$distribution_tier"
  printf '  "workers": [\n'
  printf '    {"architecture": "arm64", "backend": "unicorn", "path": "arm64/aex-guest-worker", "sha256": "%s", "size": %s}' "$arm64_sha" "$arm64_size"
  if [ "$include_native" = "1" ]; then
    native_sha=$(shasum -a 256 "$payload/x86_64/aex-guest-worker" | awk '{print $1}')
    native_size=$(stat -f %z "$payload/x86_64/aex-guest-worker")
    printf ',\n    {"architecture": "x86_64", "backend": "native-carrier-trusted-only", "path": "x86_64/aex-guest-worker", "sha256": "%s", "size": %s}' "$native_sha" "$native_size"
  fi
  printf '\n  ]\n}\n'
} >"$payload/manifest.json"

mkdir -p "$(dirname -- "$output")"
hdiutil create -quiet -fs HFS+ -format UDZO -volname "AEXCompat Carriers" \
  -srcfolder "$payload" "$output"

echo "Created macOS carrier distribution: $output"
if [ "$distribution_tier" = "developer-id" ]; then
  echo "Submit, staple, and assess: $root/tools/notarize-macos-aex-carriers.sh '$output'"
else
  echo "Local-only package: ad-hoc signatures are not notarized or Gatekeeper-approved for distribution"
fi
