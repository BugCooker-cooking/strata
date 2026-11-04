#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

portable_crates="${STRATA_PORTABLE_CRATES:-crates/strata-pixels crates/strata-core crates/strata-color crates/strata-io crates/strata-ai}"
patterns='target_os[[:space:]]*=[[:space:]]*"macos"|objc|metal|core-foundation|core_foundation|cocoa|windows-sys|winapi'
found=0
for crate in $portable_crates; do
    for entry in "$crate/src" "$crate/tests" "$crate/Cargo.toml"; do
        if [[ -e "$entry" ]]; then
            if grep -nER "$patterns" "$entry"; then
                found=1
            else
                status=$?
                if [[ "$status" -ne 1 ]]; then exit "$status"; fi
            fi
        fi
    done
done
if [[ "$found" -ne 0 ]]; then exit 1; fi
echo "portability check passed"
