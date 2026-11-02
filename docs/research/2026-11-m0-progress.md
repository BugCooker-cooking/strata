# M0 实施进度与验证记录

日期：2026-11-02（Asia/Taipei）。状态：**可运行原型已落地，M0 尚未全部验收**。

Task 1–9 首次实现以统一检查点入库，后续增量提交。已按发起人授权关联并推送 [BugCooker-cooking/strata](https://github.com/BugCooker-cooking/strata)，核对为私有仓库；成品完成前保持私有。没有发布产品。

## 已实现

- 本地 Git 仓库、Rust workspace、Apache-2.0、贡献/行为准则、Issue/PR 模板。
- `strata-pixels`：8/16 位缓冲、位深推导、尺寸/样本数/溢出校验。
- `strata-color`：Display P3 ICC、8/16 位转换、坏 ICC 回退、不兼容 RGB 的配置报错、alpha 保留。
- `strata-io`：按内容识别 PNG/JPEG/WebP/TIFF/GIF/BMP、ICC、16 位 PNG/TIFF 与解码预算；GIF 仅首帧。
- `strata-core`：不可变源像素文档、提交后事件值；回调不在 core 持锁期间执行。
- `strata-render`：视图数学、线性预乘浮点显示缓存、离屏读回、真实 CAMetalLayer 表面、P3 surface 配置、零尺寸与释放。
- `strata-ffi`：UniFFI 绑定、锁外有序事件、generation 淘汰旧请求、GPU 准备成功后提交文档、画布 attach/detach/平移/缩放。
- AppKit 最小窗口：打开图片、原始信息/位深、实色画布、拖动/滚动/捏合、缩放菜单、适合屏幕、错误提示。
- `make run/test/lint/test-core/test-gpu`、XcodeGen、Rust 静态链接、Linux/macOS 工作流、24MP 与 P3 样例生成脚本。
- `make color-fixtures`：可重复生成的原生 P3 色块、完整 16 位灰阶、透明边缘及预期值；3 个集成测试。
- `make benchmark`：真实 Release 画布的 60Hz 固定命令轨迹，3×30 秒与原始时间戳、CPU/内存/环境报告；不丢弃卡顿，中断不能写成功报告。
- `make verify-clean`：精确 HEAD 源码导出，验证无编译缓存、生成绑定和 Xcode 工程时仍可构建测试。

## 本机证据

| 检查 | 结果与范围 |
|---|---|
| Rust 工具链 | 1.97.1；候选版本已改为精确 patch pin |
| 原生工具链 | Xcode 27.1（27A9275）、Swift 6.4、XcodeGen 2.45.4 |
| 机器 | M4 Mac mini、16 GiB，macOS 27.0.1；不是计划中的 M1 MacBook Air 性能基准机 |
| Rust | M0 基线 56 个 workspace 测试通过；M1a 新增分块测试后共 66 个，见后续实施记录。clippy `-D warnings`、rustfmt、可移植扫描通过；GPU 验收另设强制模式，不能用跳过冒充通过 |
| GPU | `make test-gpu`：7 个 FFI、11 个视图数学、8 个真实 Metal 离屏测试通过，包括上传失败保留原状态、16 位源不变、透明边缘无色晕 |
| Swift | Swift 6 严格并发编译通过，无需降级；7 个 XCTest（4 个桥接/画布、3 个帧统计）；0 失败、0 跳过。含 CAMetalLayer attach/零尺寸 resize/detach、文档保留、200ms 卡顿不能被过滤 |
| 原生 UI | 已确认示例照片、菜单缩放/拖动/适合屏幕、P3/16 位夹具显示、透明边缘、损坏文件失败保留原画面，以及三次关闭重开；[实机记录](2026-11-m0-hardware-checklist.md)保留触控板、最小化/跨屏和对色缺口 |
| 素材脚本 | `build/perf-24mp.jpg` 确认为 4000×6000；`build/color-fixtures-v1` 包含原生 P3/16 位/alpha 夹具；sRGB→P3 照片也已生成 |
| 干净源码构建 | 提交 `2c46afe` 的 `git archive`，在 `build/clean-check.YPay7S` 完成 `make test`：56 Rust + 7 XCTest；仅复用下载的 registry 源码，不复用 target、绑定、工程或 DerivedData。日志 `validation.log` 与 xcresult 保留在该目录 |
| Release | arm64 原生应用构建成功；Swift 优化与 Rust release profile 均启用 |
| 24MP 基准 | 三轮 59.88 / 59.87 / 60.03 fps，p95 16.93 / 16.92 / 16.96ms；本机 60Hz 阈值通过，详见[完整条件与限制](2026-11-m0-rendering-wgpu-metal.md) |

首次 GPU 在沙盒内返回 NoAdapter，升级为本机 Metal 验证后通过。Xcode 测试也需要系统缓存/测试服务权限；没有把沙盒失败记成产品通过。完整结果可用上方命令复验。

## 远程 CI 证据

提交 `32fb272`、`27b1b6f`、`0624958`、`e80cbf1` 的 Linux/macOS job 均通过；本轮源码验证为 [41421973474](https://github.com/BugCooker-cooking/strata/actions/runs/41421973474)。Ubuntu 24.04 验证核心与静态检查；macOS 15 arm64 从未生成工程的 checkout 执行 `make test`，未恢复编译缓存。已实测 Xcode 16.4（16F6）、Swift 6.1.2、XcodeGen 2.46.0。

工作流显式选择 Xcode 16.4，下载并校验官方 XcodeGen 2.46.0 发布摘要；增加 Release 构建回归与 XCTest 结果归档。这三项均已在 `e80cbf1` 的远程运行通过。Hosted runner 的无 GPU 跳过不算本机 Metal 验收，macOS 15 CI 不算 macOS 14 实机验证。

## 与计划的必要接口调整

1. `display_p3_profile()` 返回 `Result`，避免把色彩配置构造失败变成 panic。像素错误复用 `PixelError`；错误枚举增加资源上限与内部状态错误。
2. `ImageDocument::new` 校验像素并从缓冲推导位深，字段不公开可变；无法构造“16 位标记配 8 位数据”的有效文档。
3. 相同 ICC 的有限精度往返会损失部分相邻 16 位码值：仅忽略创建时间/校验摘要后配置完全一致才走保真复制；其他配置仍实际转换。16 位转换与 float 参考量化误差 ≤ 2/65535。
4. `ImageReader::open().with_guessed_format()` 在内容无识别结果时会保留扩展名 hint；改从 `File + BufReader + ImageReader::new` 开始，保证假 JPG 的文本得到 Unsupported，而不是误选 JPEG decoder。
5. wgpu 30 使用 `InstanceDescriptor::new_without_display_handle()`、`ErrorScopeGuard::pop()`、`CurrentSurfaceTexture` 和 `Queue::present()`；surface 明确设置 `DisplayP3`，不依赖 layer 默认值。
6. `commit_image` 先检查候选所属 renderer，返回 Result；检查通过后不做可失败资源准备。resize 与 FFI 视图方法返回错误，不吞掉表面配置失败。
7. Swift 保留源资源文件名 `sample-photo.jpg`，无需自定义重命名构建阶段。Rust staticlib 显式链接；生成器用 dylib，但应用不依赖项目目录中的 dylib。
8. macOS 自带 Bash 3.2 在 `set -u` 下展开空数组会报错：构建脚本使用显式 Debug/Release 分支。
9. 本机 Rust 1.97.1 的 Release 构建出现 host proc-macro 元数据加载失败；设置 `[profile.release.build-override] debug = true` 后通过，仅作用于构建依赖，运行时代码仍优化。尚未确定更底层的工具链原因。
10. M0 明确设 `ARCHS: arm64`，避免 Xcode Release 默认通用架构错误地链接仅 arm64 的 Rust 静态库；通用产物留在 M8。
11. 原生 XCTest 宿主启动不自动创建产品窗口，无 GPU 时仅跳过需要 adapter 的测试；`STRATA_REQUIRE_GPU=1` 可强制缺 GPU 失败。基准模式可置顶，报告注明窗口模式，并继续检查遮挡、视口、屏幕和用户输入。

## 尚未完成的 M0 门槛

- 与“预览”/数码测色计在同一 P3 屏幕对色。夹具已完成；显示截图不能证明色彩正确。
- 最低 macOS 14 运行、Retina/非 Retina 跨屏、持续触控板平移/捏合、窗口最小化/恢复回归。
- M1 MacBook Air 标准机性能复验；当前主机的程序驱动结果不能替代标准机或真实触控板体验。
- 行为准则私密联系方式在开放外部贡献前补齐，不阻塞私有仓库的已授权工作。

## 下一步

发起人已确认先用当前 Mac，其他设备暂缺。按[实机验收表](2026-11-m0-hardware-checklist.md)保留 M0 缺口，同时执行不依赖额外设备的 [M1a 分块/命令/保存基础](../plans/2026-11-02-m1a-core-storage.md)；这不代表 M0 全部通过。新增分块测试与工作状态见 [M1a 进度](2026-11-m1a-progress.md)。第一版编辑、保存、图层与 AI 仍未对用户开放。
