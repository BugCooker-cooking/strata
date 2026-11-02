#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p build
sips --matchTo "/System/Library/ColorSync/Profiles/Display P3.icc" \
    docs/design/assets/sample-photo.jpg --out build/sample-p3.jpg
echo "This sRGB-to-P3 sample validates ICC loading, not out-of-sRGB gamut coverage."
