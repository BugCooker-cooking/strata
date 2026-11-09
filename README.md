# Strata

Strata 是一个开源的、可以完全由 AI 驱动的专业级图像编辑器。第一版面向 macOS，核心使用可移植的 Rust。

Strata is an open-source, AI-operable image editor with a portable Rust core and a native macOS shell. It is under active development and is not ready for editing real projects.

![v1 interface concept](docs/design/screenshots/ui-v1.png)

## 当前状态

目前在 [BugCooker-cooking/strata](https://github.com/BugCooker-cooking/strata) 私有开发；成品完成后再决定公开，不自动切换仓库可见性。

当前已推进至 M7：在可移植AI、选区/蒙版/图层/调整基础上，已增加绘画与修复、基础滤镜及高反差保留、配置/位深与图像几何、原生裁剪/拉直/自由变换手柄、自动RGB校正，以及保留源定义的横排点/段落文字。原生应用和AI共用命令执行器，保留8/16位源样本、工作ICC、撤销和工程恢复。最近本机增量证据见[文字](docs/research/2026-11-m7-editable-text-progress.md)、[变换手柄](docs/research/2026-11-m7-transform-handles-progress.md)及[完整跟踪表](docs/plans/photoshop-feature-coverage.md)。

完整范围继续覆盖PS功能清单P0–P3，尚未全部实现。一般可编辑类型的几何、连续变形像素反馈、富文本/字体、矢量路径/形状、智能对象、PSD/RAW及其余专业编辑仍继续开发；实时性能、困难照片质量、真实模型与外部设备门槛独立追踪。当前统一目录为140条真实命令及209个参数示例；最终[逐功能AI入口审计](docs/plans/ai-editing-entrypoint-coverage.md)仍未闭环，数量不代表完整覆盖。上图是v1界面设计稿，当前仍为开发版本。2026-11-05起仅本机验证，云端工作流保持禁用；下面早期CI链接只保留历史证据。

最近新增了保留原文的文字几何、嵌套图层组变换，以及持久参考线、标尺开关和移动吸附；核心、AI调用、原生测试、真实Metal、Release及单窗口增量验收已通过，示例已恢复并关闭。见[文字几何](docs/research/2026-11-m7-text-geometry-progress.md)和[参考线/吸附](docs/research/2026-11-m7-guides-snap-progress.md)；当前没有遗留测试实例。新增[图层智能参考线](docs/research/2026-11-m7-smart-guides-progress.md)与[标尺角标原点拖动/重置](docs/research/2026-11-m7-ruler-origin-drag-progress.md)通过本机自动化与Release，原生112项中111通过/1联网测试按配置跳过。用户解锁后，同一个Release窗口验证了V/数值实际吸附、小数原点/画布外取消/双击重置，恢复干净示例并退出，进程0；[单窗口范围记录](docs/research/data/2026-11-09-m7-smart-origin-native-closure.json)明确未观察的拖动中辅助线外观/延迟与持续大图门槛。

2026-11-09新增[快速导出预设](docs/research/2026-11-m8-quick-export-progress.md)：跨文档记住格式/尺寸/色域等设置，UI与AI共用真实导出命令。完整本机core354+doc1、AI51及原生/Metal/Release/示例检查通过；手工发现的尺寸框与弹窗快捷键问题已修复并重测，实际JPEG/PNG输出、原图/默认状态恢复与退出已确认。

2026-11-03 已落实原生工作区框架：文档标签、分组工具栏、属性/调整/历史、图层缩略图、可调侧栏、AI 未接入面板、标尺/缩放/导航与状态栏，支持深/浅主题。M1b 已推进独立文档标签、最近文件和可撤销的图片置入；M1b 的后续交付按分批验收记录推进。见 [工作区与 M1b 记录](docs/research/2026-11-workspace-progress.md)。

M1b 第二批已支持 JPEG（有损、质量与背景选择）及 WebP（无损/有损、质量、透明度与 DPI）导出，所有输出可保留工作 ICC 或转换为 sRGB/P3；16 位转 JPEG/WebP 必须显式允许量化。导出不修改原文档。第二批验收见 [导出验收](docs/research/2026-11-export-options-progress.md)。

M1b 第三批已接入导出尺寸、macOS HEIC、30 秒空闲恢复检查点与启动恢复入口、跨标签的像素/显示资源预算及磁盘后备；50MP × 30 独立随机 16 位图层完成像素转换、撤销、恢复及逐样本保存重开压力验证。当前执行范围已闭环；保留的外部设备验收与格式边界见 [第三批记录](docs/research/2026-11-m1b-recovery-memory-progress.md)。

收尾实现已通过精确源码独立构建（146 Rust + 23 XCTest）；`29fc5ac` 的 [Linux/macOS CI](https://github.com/BugCooker-cooking/strata/actions/runs/41804367031) 全绿，含 macOS Release。完整 [闭环清单](docs/plans/2026-11-03-m1-closure.md)保留逐项证据和设备依赖。

当前 Mac 已通过 8/16 位 24MP×20 的三轮原生帧率（约 60fps、p95≤18.50ms）、JPEG 1.5 秒打开参考目标、恢复另存及多文档缓存压力；完整条件见 [原生验收](docs/research/2026-11-m1b-native-acceptance.md)。

当前 P3 同屏六色与照片已由用户实测确认一致。M0 额外设备与物理输入验收继续追踪；当前 Mac 的 M1a 验收证据、容量限制与 M1b 边界见 [M1a 实施记录](docs/research/2026-11-m1a-progress.md)。

## 开发

- Rust **1.97.1**（rustup 自动读取 `rust-toolchain.toml`）。
- macOS 本机验证环境：Apple Silicon、Xcode **27.2** / Swift **6.4.2**、XcodeGen **2.45.4**。
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
cargo run --release -p strata-devtool -- selection-benchmark build/new-selection-report.json
cargo run --release -p strata-devtool -- tonal-benchmark build/new-tonal-report.json
cargo run --release -p strata-devtool -- tonal-benchmark build/new-local-tone-report.json local
```

`make run` 自动编译 Rust、生成 Swift 绑定与 Xcode 工程并启动应用。`make test` 同时跑 Rust 和 XCTest；Linux 使用 `make test-core`。`make test-gpu` 要求真实 GPU，不允许以无 adapter 跳过代替通过。Release 使用 `make run CONFIGURATION=Release FRAME_STATS=1`。

本机验收一次只运行一个 Strata 实例，完成即退出；Make 默认两个构建任务。选区/调色基准记录固定 24MP / 16 位命令或完整 CPU 导出耗时，不作为原生帧率证据。

`make color-fixtures` 生成原生 P3 色块、完整 16 位灰阶和透明边缘，拒绝覆盖已有输出；再次生成时指定 `FIXTURE_OUTPUT=build/color-fixtures-v2`。`make benchmark` 运行真实 Release 画布，置顶诊断窗口完成 3×30 秒平移缩放，原始时间戳和环境写入独立的 `build/canvas-benchmark.*/report.json`；期间请勿操作该窗口，遮挡、尺寸/屏幕变化或用户输入会中止测量。报告记录置顶模式、60Hz 请求频率与屏幕实际刷新能力，不把 CPU 提交当作 GPU 呈现时间。

`make verify-clean` 要求工作区已提交，从精确 HEAD 导出无 `target`、绑定和 Xcode 工程的源码，在独立目录执行 `make test`，只复用已下载的依赖源码。它不删除当前构建，也不修改工作区。

打开 PNG/JPEG/WebP/TIFF/GIF/BMP（GIF 首帧）、macOS HEIC（主图、RGB，整数 8/10/16 位）或 `.strata`；浮点/HDR 增益图、深度辅助图和不兼容的灰度/CMYK ICC 仍不支持。源数据按 256×256 分块，显示缓存不回写源图。默认跨文档 CPU 像素/缓存/暂存预算 2 GiB、GPU 资源估算 512 MiB、临时磁盘后备 32 GiB；这不是整个进程的 RSS 上限。单文档源块逻辑限 16 GiB、128 层，历史最多 64 条/24 GiB 独立引用；解码、画板及平面导出仍限 512 MiB，另受全局预算约束。工程读写逐块进行。见 [工程格式与命令](docs/research/2026-11-m1a-format-and-commands.md)。

## 文档与贡献

[文档入口](docs/README.md) · [实施计划](docs/plans/2026-11-02-strata-v1-roadmap.md) · [贡献指南](CONTRIBUTING.md) · [行为准则](CODE_OF_CONDUCT.md)

许可证：[Apache-2.0](LICENSE)。依赖保留其原许可证；发布前生成第三方许可清单。远程保持私有，CI 状态以实际运行记录为准。
