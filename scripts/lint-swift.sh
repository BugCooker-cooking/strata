#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -s)" != Darwin ]]; then
    echo 'Native SwiftLint gate runs in the macOS CI job.'
    exit 0
fi
tool_version=0.59.1
tool_directory="$PWD/build/tools/swiftlint-$tool_version"
lint_binary="${STRATA_SWIFTLINT_BIN:-$tool_directory/swiftlint}"
if [[ ! -x "$lint_binary" ]]; then
    mkdir -p "$tool_directory"
    archive="$tool_directory/portable_swiftlint.zip"
    curl --fail --location --retry 3 \
        "https://github.com/realm/SwiftLint/releases/download/$tool_version/portable_swiftlint.zip" --output "$archive"
    printf '%s  %s\n' '58f9be8a4677900c945e2c618168223f4dd620a0cc65c9ccc5ea0f70433e89c1' "$archive" | shasum -a 256 -c -
    unzip -q -o "$archive" -d "$tool_directory"
fi
[[ "$("$lint_binary" version)" == "$tool_version" ]]
"$lint_binary" lint --strict --no-cache --quiet --config .swiftlint.yml
