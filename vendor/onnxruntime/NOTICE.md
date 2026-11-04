# ONNX Runtime used for local subject selection

- ONNX Runtime 1.22.0, MIT, Microsoft Corporation and contributors.
- macOS arm64 static package distributed by the ort Rust binding maintainers:
  https://cdn.pyke.io/0/pyke:ort-rs/ms@1.22.0/aarch64-apple-darwin.tgz
- Archive SHA-256: 00fbfd6f08bac2a4e28c66723af900d58d1b4b1c73efba6290637cd3019883d5.
- Static library SHA-256: e5c83560aa9e88afa39d9dca9fb5f5a767e28adb5458d1c36fe0357131b6af8b.
- The distribution includes the macOS exit logging mutex fix from
  pykeio/ort-artifacts (0003-leak-logger-mutex.patch). Unpatched Microsoft
  1.22.0 reproduced an exit abort locally and is not used by Strata.
- Strata links this archive as an app-local dynamic library targeting macOS 14,
  signs the embedded copy and records its exact hash in the app resources.
- Upstream source: https://github.com/microsoft/onnxruntime
- Runtime license and bundled third party notices are included alongside this
  notice. Telemetry is disabled during environment initialization.

The U²-Net-P ONNX model is a separate optional user-approved download. Its
upstream source is https://github.com/xuebinqin/U-2-Net (Apache-2.0), with the
ONNX conversion distributed by https://github.com/danielgatis/rembg (MIT).
The fixed URL, version and checksum are defined in strata-ml::assets.
