#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p build
cargo run --quiet --locked -p strata-devtool -- docs build/command-reference.current.md
diff -u docs/commands/reference.md build/command-reference.current.md
