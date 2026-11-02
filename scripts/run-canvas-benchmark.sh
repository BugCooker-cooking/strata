#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# This runs the actual native application and display-link path, not an offscreen loop.
# A floating diagnostic window avoids accidental occlusion by other ordinary windows.
# Do not interact with it during the run; occlusion/interruption checks remain enabled.
if [[ ! -f build/perf-24mp.jpg ]]; then bash scripts/make-perf-image.sh; fi
make build CONFIGURATION=Release
report_dir=$(mktemp -d "$PWD/build/canvas-benchmark.XXXXXX")
export STRATA_SOURCE_REVISION
STRATA_SOURCE_REVISION=$(git rev-parse HEAD)
if [[ -n "$(git status --porcelain)" ]]; then STRATA_SOURCE_REVISION="$STRATA_SOURCE_REVISION-dirty"; fi
printf 'Benchmark output: %s\n' "$report_dir"
build/DerivedData/Build/Products/Release/Strata.app/Contents/MacOS/Strata \
    --benchmark-input "$PWD/build/perf-24mp.jpg" \
    --benchmark-output "$report_dir/report.json" \
    --benchmark-seconds 30 --benchmark-runs 3 --benchmark-keep-visible > "$report_dir/application.log" 2>&1
if [[ ! -f "$report_dir/report.json" ]]; then
    tail -n 30 "$report_dir/application.log"
    echo 'Benchmark failed or was interrupted; no success report was written.' >&2
    exit 1
fi
ruby -rjson -e '
  r = JSON.parse(File.read(ARGV[0]))
  puts JSON.pretty_generate(r.reject { |k, _| k == "runs" }.merge("runSummaries" => r.fetch("runs").map { |v| v.fetch("summary") }))
  exit(r.fetch("qualifiesAsM0HostRun") ? 0 : 1)
' "$report_dir/report.json"
