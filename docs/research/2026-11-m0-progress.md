# M0 实施进度与验证记录

日期：2026-11-02（Asia/Taipei）。状态：**可运行原型已落地，M0 尚未全部验收**。

本轮将 Task 1–9 的已实现源码/测试与计划一起保存为统一的本地 M0 检查点，未按原计划拆成逐任务微提交；后续继续小步提交。远程创建/推送仍未执行。

## 已实现

- 本地 Git 仓库、Rust workspace、Apache-2.0、贡献/行为准则、Issue/PR 模板。
- `strata-pixels`：8/16 位缓冲、位深推导、尺寸/样本数/溢出校验。
- `strata-color`：Display P3 ICC、8/16 位转换、坏 ICC 回退、不兼容 RGB 的配置报错、alpha 保留。
- `strata-io`：按内容识别 PNG/JPEG/WebP/TIFF/GIF/BMP、ICC、16 位 PNG/TIFF 与解码预算；GIF 仅首帧。
- `strata-core`：不可变源像素文档、提交后事件值；回调不在 core 持锁期间执行。
- `strata-render`：视图数学、线性预乘浮点显示缓存、离屏读回、真实 CAMetalLayer 表面、P3 surface 配置、零尺寸与释放。
- `strata-ffi`：UniFFI 绑定、锁外有序事件、generation 淘汰旧请求、GPU 准备成功后提交文档、画布 attach/detach/平移/缩放。
- AppKit 最小窗口：打开图片、原始信息/位深、实色画布、拖动/滚动/捏合、缩放菜单、适合屏幕、错误提示。
- `make run/test/lint/test-core/test-gpu`、XcodeGen、Rust 静态链接、Linux/macOS 工作流配置、24MP 与 P3 样例生成脚本。

## 本机证据

| 检查 | 结果与范围 |
|---|---|
| Rust 工具链 | 1.97.1；候选版本已改为精确 patch pin |
| 原生工具链 | Xcode 27.1（27A9275）、Swift 6.4、XcodeGen 2.45.4 |
| 机器 | arm64 Mac mini，macOS 27.0.1；不是计划中的 M1 MacBook Air 性能基准机 |
| Rust | 53 个 workspace 测试通过；clippy `-D warnings`、rustfmt、可移植扫描通过；GPU 验收另设强制模式，不能用跳过冒充通过 |
| GPU | `make test-gpu`：7 个 FFI、11 个视图数学、8 个真实 Metal 离屏测试通过，包括上传失败保留原状态、16 位源不变、透明边缘无色晕 |
| Swift 桥接 | Swift 6 严格并发编译通过，无需降级；统一 `make test` 已通过 4 个 XCTest，含 CAMetalLayer attach/零尺寸 resize/detach、文档保留；0 失败、0 跳过 |
| 原生 UI | 已通过 UI 截图/交互确认示例照片显示、菜单放大、鼠标拖动与适合屏幕；触控板手势、Retina 跨屏未实测 |
| 素材脚本 | `build/perf-24mp.jpg` 确认为 4000×6000；sRGB→P3 照片生成成功，但不等于原生广色域验证 |

首次 GPU 在沙盒内返回 NoAdapter，升级为本机 Metal 验证后通过。Xcode 测试也需要系统缓存/测试服务权限；没有把沙盒失败记成产品通过。完整结果可用上方命令复验。

## 与计划的必要接口调整

1. `display_p3_profile()` 返回 `Result`，避免把色彩配置构造失败变成 panic。像素错误复用 `PixelError`；错误枚举增加资源上限与内部状态错误。
2. `ImageDocument::new` 校验像素并从缓冲推导位深，字段不公开可变；无法构造“16 位标记配 8 位数据”的有效文档。
3. 相同 ICC 的有限精度往返会损失部分相邻 16 位码值：仅忽略创建时间/校验摘要后配置完全一致才走保真复制；其他配置仍实际转换。16 位转换与 float 参考量化误差 ≤ 2/65535。
4. `ImageReader::open().with_guessed_format()` 在内容无识别结果时会保留扩展名 hint；改从 `File + BufReader + ImageReader::new` 开始，保证假 JPG 的文本得到 Unsupported，而不是误选 JPEG decoder。
5. wgpu 30 使用 `InstanceDescriptor::new_without_display_handle()`、`ErrorScopeGuard::pop()`、`CurrentSurfaceTexture` 和 `Queue::present()`；surface 明确设置 `DisplayP3`，不依赖 layer 默认值。
6. `commit_image` 先检查候选所属 renderer，返回 Result；检查通过后不做可失败资源准备。resize 与 FFI 视图方法返回错误，不吞掉表面配置失败。
7. Swift 保留源资源文件名 `sample-photo.jpg`，无需自定义重命名构建阶段。Rust staticlib 显式链接；生成器用 dylib，但应用不依赖项目目录中的 dylib。
8. macOS 自带 Bash 3.2 在 `set -u` 下展开空数组会报错：构建脚本使用显式 Debug/Release 分支。

## 尚未完成的 M0 门槛

- Release 24MP 连续交互 3×30 秒、fps/p50/p95/内存实测；当前 HUD 的零散帧数不是持续性能基准。
- 原生超 sRGB 色块 + 16 位渐变夹具生成器、与“预览”/数码测色计在同一 P3 屏幕对色。显示截图不能证明色彩正确。
- 真正从无 target/Generated/.xcodeproj 的干净检出完成 `make test`；目前依赖顺序已写对，首次构建部分生成物已存在。
- 最低 macOS 14 运行、Retina/非 Retina 跨屏、持续触控板平移/捏合、窗口最小化/恢复回归。
- Linux/macOS GitHub Actions 实跑：本机无 Linux 容器/目标环境，远程尚未创建，工作流未跑。runner 原生工具版本仍需在首次 CI 后锁定。
- 远程账号/仓库名/公开范围与行为准则私密联系方式确认，之后才创建/推送；没有发布产品。

## 下一步

先补齐上述 M0 技术验收与故障注入，再准备 M1a 的命令/分块/保存任务；不因已有窗口就跳过存储与高位深门槛。第一版编辑、保存、图层与 AI 全部仍属于后续里程碑。
