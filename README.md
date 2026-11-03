# Strata

Strata 是一个开源的、可以完全由 AI 驱动的专业级图像编辑器。第一版面向 macOS，核心使用可移植的 Rust。

Strata is an open-source, AI-operable image editor with a portable Rust core and a native macOS shell. It is under active development and is not ready for editing real projects.

![v1 interface concept](docs/design/screenshots/ui-v1.png)

## 当前状态

目前在 [BugCooker-cooking/strata](https://github.com/BugCooker-cooking/strata) 私有开发；成品完成后再决定公开，不自动切换仓库可见性。

M1a 已接入单画板与像素图层：新建/打开、增删复制/排序/移动、显隐/不透明度、四种混合模式、撤销/重做、`.strata` 保存重开、PNG/TIFF 8/16 位导出，以及显式工作空间转换。原生应用与无界面工具使用同一命令执行器。上图是完整 v1 的界面设计稿，不是当前功能截图。尚未接入 AI 模型、选区、蒙版或调整层，不是正式产品。

2026-11-03 已落实原生工作区框架：文档标签、分组工具栏、属性/调整/历史、图层缩略图、可调侧栏、AI 未接入面板、标尺/缩放/导航与状态栏，支持深/浅主题。M1b 已推进独立文档标签、最近文件和可撤销的图片置入；M1b 的后续交付按分批验收记录推进。见 [工作区与 M1b 记录](docs/research/2026-11-workspace-progress.md)。

M1b 第二批已支持 JPEG（有损、质量与背景选择）及 WebP（无损/有损、质量、透明度与 DPI）导出，所有输出可保留工作 ICC 或转换为 sRGB/P3；16 位转 JPEG/WebP 必须显式允许量化。导出不修改原文档。第二批验收见 [导出验收](docs/research/2026-11-export-options-progress.md)。

M1b 第三批已接入导出尺寸、macOS HEIC、30 秒空闲恢复检查点与启动恢复入口、跨标签的像素/显示资源预算及磁盘后备；50MP × 30 独立 16 位图层完成逐样本保存重开压力验证。完整 M1b 仍未关闭，剩余 CI、设备与显示性能门槛见 [第三批记录](docs/research/2026-11-m1b-recovery-memory-progress.md)。

M0 额外设备与人工显示/输入验收继续追踪；当前 Mac 的 M1a 验收证据、容量限制与 M1b 边界见 [M1a 实施记录](docs/research/2026-11-m1a-progress.md)。

## 开发

- Rust **1.97.1**（rustup 自动读取 `rust-toolchain.toml`）。
- macOS 本机验证环境：Apple Silicon、Xcode **27.1** / Swift **6.4**、XcodeGen **2.45.4**。
- 远程已通过：Ubuntu 24.04、macOS 15 arm64；原生环境 Xcode **16.4** / Swift **6.1.2**、XcodeGen **2.46.0**。
- 最终应用最低运行版本目标为 macOS **14.0**；该最低版本尚未实机验证。

```sh
make run
make test
make lint
make test-gpu
make color-fixtures
make benchmark
make verify-clean
make commands
cargo run --release -p strata-devtool -- demo build/m1a-demo-new
cargo run --release -p strata-devtool -- capacity
cargo run --release -p strata-devtool -- memory-stress build/new-memory-stress
```

`make run` 自动编译 Rust、生成 Swift 绑定与 Xcode 工程并启动应用。`make test` 同时跑 Rust 和 XCTest；Linux 使用 `make test-core`。`make test-gpu` 要求真实 GPU，不允许以无 adapter 跳过代替通过。Release 使用 `make run CONFIGURATION=Release FRAME_STATS=1`。

`make color-fixtures` 生成原生 P3 色块、完整 16 位灰阶和透明边缘，拒绝覆盖已有输出；再次生成时指定 `FIXTURE_OUTPUT=build/color-fixtures-v2`。`make benchmark` 运行真实 Release 画布，置顶诊断窗口完成 3×30 秒平移缩放，原始时间戳和环境写入独立的 `build/canvas-benchmark.*/report.json`；期间请勿操作该窗口，遮挡、尺寸/屏幕变化或用户输入会中止测量。报告记录置顶模式、60Hz 请求频率与屏幕实际刷新能力，不把 CPU 提交当作 GPU 呈现时间。

`make verify-clean` 要求工作区已提交，从精确 HEAD 导出无 `target`、绑定和 Xcode 工程的源码，在独立目录执行 `make test`，只复用已下载的依赖源码。它不删除当前构建，也不修改工作区。

打开 PNG/JPEG/WebP/TIFF/GIF/BMP（GIF 首帧）、macOS HEIC（主图、RGB，整数 8/10/16 位）或 `.strata`；浮点/HDR 增益图、深度辅助图和不兼容的灰度/CMYK ICC 仍不支持。源数据按 256×256 分块，显示缓存不回写源图。默认跨文档 CPU 像素/缓存/暂存预算 2 GiB、GPU 资源估算 512 MiB、临时磁盘后备 32 GiB；这不是整个进程的 RSS 上限。单文档源块逻辑限 16 GiB、128 层，历史最多 64 条/24 GiB 独立引用；解码、画板及平面导出仍限 512 MiB，另受全局预算约束。工程读写逐块进行。见 [工程格式与命令](docs/research/2026-11-m1a-format-and-commands.md)。

## 文档与贡献

[文档入口](docs/README.md) · [实施计划](docs/plans/2026-11-02-strata-v1-roadmap.md) · [贡献指南](CONTRIBUTING.md) · [行为准则](CODE_OF_CONDUCT.md)

许可证：[Apache-2.0](LICENSE)。依赖保留其原许可证；发布前生成第三方许可清单。远程保持私有，CI 状态以实际运行记录为准。
