#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ -n "$(git status --porcelain)" ]]; then
    echo 'Commit the source first: clean-build verification tests an exact HEAD archive.' >&2
    exit 2
fi
mkdir -p build
clean_root=$(mktemp -d "$PWD/build/clean-check.XXXXXX")
revision=$(git rev-parse HEAD)
git archive HEAD | tar -x -C "$clean_root"
test ! -e "$clean_root/target"
test ! -e "$clean_root/apps/macos/Generated"
test ! -e "$clean_root/apps/macos/Strata.xcodeproj"
printf 'Clean source revision: %s\nSource directory: %s\n' "$revision" "$clean_root"
# Reuse downloaded registry sources only, never existing compilation or generated app artifacts.
unset CARGO_TARGET_DIR
export CARGO_NET_OFFLINE=true
if make -C "$clean_root" test > "$clean_root/validation.log" 2>&1; then
    printf 'PASS: clean source build + tests at %s\n' "$clean_root"
    tail -n 30 "$clean_root/validation.log"
else
    tail -n 70 "$clean_root/validation.log"
    exit 1
fi
