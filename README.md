# Strata

Strata 是一个开源的、可以完全由 AI 驱动的专业级图像编辑器。第一版面向 macOS，核心使用可移植的 Rust。

Strata is an open-source, AI-operable image editor with a portable Rust core and a native macOS shell. It is under active development and is not ready for editing real projects.

![v1 interface concept](docs/design/screenshots/ui-v1.png)

## 当前状态

M0 开发中。已有可运行的原生只读图像预览：打开常见图片、平移/缩放、8/16 位源像素保留、ICC → Display P3 显示、错误处理。上图是完整 v1 的界面设计稿，不是当前功能截图。尚无编辑、图层、保存或 AI；不要把它当作正式编辑器。

## 开发

- Rust **1.97.1**（rustup 自动读取 `rust-toolchain.toml`）。
- macOS 本机验证环境：Apple Silicon、Xcode **27.1** / Swift **6.4**、XcodeGen **2.45.4**。
- 最终应用最低运行版本目标为 macOS **14.0**；该最低版本尚未实机验证。

```sh
make run
make test
make lint
make test-gpu
```

`make run` 自动编译 Rust、生成 Swift 绑定与 Xcode 工程并启动应用。`make test` 同时跑 Rust 和 XCTest；Linux 使用 `make test-core`。`make test-gpu` 要求真实 GPU，不允许以无 adapter 跳过代替通过。Release 使用 `make run CONFIGURATION=Release FRAME_STATS=1`。

M0 打开 PNG/JPEG/WebP/TIFF/GIF/BMP（GIF 首帧），HEIC 与不兼容的灰度/CMYK ICC 暂不支持。最终 RGBA 解码缓冲限 512 MiB，图像边长受 GPU 限制，M1 才分块。屏幕缓存可降精度，但不回写源图；没有保存功能。详细证据和未完成门槛见 [M0 进度](docs/research/2026-11-m0-progress.md)。

## 文档与贡献

[文档入口](docs/README.md) · [实施计划](docs/plans/2026-11-02-strata-v1-roadmap.md) · [贡献指南](CONTRIBUTING.md) · [行为准则](CODE_OF_CONDUCT.md)

许可证：[Apache-2.0](LICENSE)。依赖保留其原许可证；发布前生成第三方许可清单。尚未创建远程仓库，CI 云端状态未验证。
