#!/bin/sh
set -eu

if [ "$(uname -s)" != "Darwin" ]; then
  echo "notarize-macos-aex-carriers.sh requires macOS" >&2
  exit 2
fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
artifact=${1:-"$root/guest/target/aexcompat-macos-carriers.dmg"}
evidence_dir=${2:-"$root/guest/target/notarization-evidence"}
profile=${AEXCOMPAT_NOTARYTOOL_PROFILE:-}

if [ ! -f "$artifact" ]; then
  echo "missing distribution: $artifact" >&2
  exit 2
fi
if [ -z "$profile" ]; then
  echo "AEXCOMPAT_NOTARYTOOL_PROFILE must name a notarytool Keychain profile" >&2
  exit 2
fi
if [ -e "$evidence_dir" ]; then
  echo "refusing to replace existing notarization evidence: $evidence_dir" >&2
  exit 2
fi

mkdir -p "$evidence_dir"
submission="$evidence_dir/submission.json"
log="$evidence_dir/notary-log.json"

set +e
xcrun notarytool submit "$artifact" --keychain-profile "$profile" --wait \
  --output-format json >"$submission"
submit_status=$?
set -e

submission_id=$(plutil -extract id raw -o - "$submission" 2>/dev/null || true)
if [ -n "$submission_id" ]; then
  xcrun notarytool log "$submission_id" --keychain-profile "$profile" "$log" || true
fi
if [ "$submit_status" -ne 0 ]; then
  echo "notarytool submit failed; evidence retained at $evidence_dir" >&2
  exit "$submit_status"
fi

status=$(plutil -extract status raw -o - "$submission")
if [ "$status" != "Accepted" ]; then
  echo "notarization was not accepted: $status" >&2
  exit 1
fi
if [ ! -s "$log" ]; then
  echo "complete notary log was not retrieved" >&2
  exit 1
fi

xcrun stapler staple "$artifact"
xcrun stapler validate "$artifact"
spctl -a -t open --context context:primary-signature -v "$artifact"

echo "Notarization accepted, log retained, ticket stapled, and Gatekeeper assessment passed"
echo "Evidence: $evidence_dir"
