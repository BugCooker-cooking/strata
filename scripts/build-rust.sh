#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

case "${1:-Debug}" in
    Debug|debug) profile=debug ;;
    Release|release) profile=release ;;
    *) echo "usage: $0 [Debug|Release]" >&2; exit 2 ;;
esac

export MACOSX_DEPLOYMENT_TARGET=14.0
if [[ "$profile" == release ]]; then
    cargo build --locked -p strata-ffi --release
else
    cargo build --locked -p strata-ffi
fi
mkdir -p apps/macos/Generated
cargo run -q --locked -p uniffi-bindgen -- generate \
    --library "target/$profile/libstrata_ffi.dylib" \
    --language swift --out-dir apps/macos/Generated
