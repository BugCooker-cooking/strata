#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p build
sips -z 6000 4000 docs/design/assets/sample-photo.jpg --out build/perf-24mp.jpg
