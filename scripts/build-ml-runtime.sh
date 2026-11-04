#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
runtime_root="build/subject-runtime"
archive_path="$runtime_root/ort-1.22.0-arm64.tgz"
archive_hash="00fbfd6f08bac2a4e28c66723af900d58d1b4b1c73efba6290637cd3019883d5"
static_hash="e5c83560aa9e88afa39d9dca9fb5f5a767e28adb5458d1c36fe0357131b6af8b"
mkdir -p "$runtime_root"
if [[ ! -f "$archive_path" ]]; then
    if [[ -f build/ort-pyke-1.22.0-arm64.tgz ]]; then
        cp build/ort-pyke-1.22.0-arm64.tgz "$archive_path"
    else
        curl --fail --location --retry 2 --connect-timeout 15 --max-time 120 \
            --proto '=https' --proto-redir '=https' \
            'https://cdn.pyke.io/0/pyke:ort-rs/ms@1.22.0/aarch64-apple-darwin.tgz' \
            --output "$archive_path.part"
        printf '%s  %s\n' "$archive_hash" "$archive_path.part" | shasum -a 256 -c -
        mv "$archive_path.part" "$archive_path"
    fi
fi
printf '%s  %s\n' "$archive_hash" "$archive_path" | shasum -a 256 -c - >/dev/null
if [[ ! -f "$runtime_root/libonnxruntime.a" ]]; then
    tar -xOf "$archive_path" onnxruntime/lib/libonnxruntime.a > "$runtime_root/libonnxruntime.a.part"
    printf '%s  %s\n' "$static_hash" "$runtime_root/libonnxruntime.a.part" | shasum -a 256 -c -
    mv "$runtime_root/libonnxruntime.a.part" "$runtime_root/libonnxruntime.a"
fi
printf '%s  %s\n' "$static_hash" "$runtime_root/libonnxruntime.a" | shasum -a 256 -c - >/dev/null
runtime_library="$runtime_root/libstrata_onnxruntime.dylib"
if [[ ! -f "$runtime_library" || "$0" -nt "$runtime_library" ]]; then
    xcrun clang -dynamiclib -mmacosx-version-min=14.0 \
        "-Wl,-force_load,$runtime_root/libonnxruntime.a" \
        -install_name @rpath/libstrata_onnxruntime.dylib \
        -framework Foundation -framework CoreML -lc++ -o "$runtime_library.part"
    mv "$runtime_library.part" "$runtime_library"
fi
echo "verified patched ONNX Runtime 1.22.0 ready"
