# M0 项目地基与技术验证 Implementation Plan

> **执行说明**：逐任务实现、先写失败测试再实现，并记录实测结果。本文不依赖未安装的 agent 技能；是否委派遵循当前会话规则。2026-11-02 已进入实施：本机原型可运行，M0 尚未全部验收，见[进度与接口偏差](../research/2026-11-m0-progress.md)。已通过 56 个 Rust 测试与 7 个 XCTest、无生成物的干净源码构建、Linux/macOS 远程 CI；显示对色、跨屏和最低系统仍待实机验证。

首次落地采用统一的本地 M0 检查点提交，下面 Task 1–9 的“提交”步骤表示对应代码已纳入该检查点，不表示逐一产生了示例消息中的独立 commit。接口与实际实现有差异时以进度记录和测试为准。

**Goal:** 搭好 Strata 的仓库、工具链和 CI，验证桥接、单图显示和色彩/位深。保留通过测试的共享接口；M0 不宣称已经验证多层合成、大图换页或完整编辑引擎。

**Architecture:** `strata-pixels` 定义共享的 `Rgba8/Rgba16` 缓冲；`strata-io` 保位深解码与 ICC；`strata-color` 按原位深生成 Display P3 显示副本；`strata-core` 持有未经显示量化的原始文档像素；`strata-render` 准备/提交显示缓存；`strata-ffi` 编排事务式打开与事件。Swift 持有 `CAMetalLayer`，显式 attach/detach；文档真值不能从屏幕缓存恢复。

**Tech Stack（已构建）:** Rust 1.97.1（edition 2024）、uniffi 0.32.2、wgpu 30.0.1、lcms2 6.2.0、image 0.25.10，精确依赖见 Cargo.lock；本机 Swift 6.4 / Xcode 27.1 / XcodeGen 2.45.4，CI Swift 6.1.2 / Xcode 16.4 / XcodeGen 2.46.0。Linux CI 已通过；macOS 14 部署目标已编译，但最低系统运行尚未验证。

**Spec:** [总体设计](../design/2026-11-02-strata-v1-design.md)（§4 硬规则、§5 架构、§6.2–6.3 存储与渲染、§6.9 色彩）· [实施计划 §3 M0](2026-11-02-strata-v1-roadmap.md) · [界面关键点](../design/ui-v1.md)

## Global Constraints

- 许可证 Apache-2.0；每个 crate 的 `license = "Apache-2.0"`。
- 最低系统 macOS 14.0（`MACOSX_DEPLOYMENT_TARGET = 14.0`）；M0 只构建本机架构（arm64），通用二进制留到 M8。
- Rust 工具链固定为 `1.97.1`（`rust-toolchain.toml`），edition 2024，workspace resolver 3。
- 依赖版本（workspace 统一声明）：`uniffi = "0.32"`、`wgpu = "30"`、`lcms2 = "6.2"`、`image = "0.25"`（关闭默认特性，只开 `png`、`jpeg`、`webp`、`tiff`、`gif`、`bmp`）、`pollster = "1"`、`bytemuck = "1"`、`thiserror = "2"`。新增依赖必须与 Apache-2.0 兼容。
- 硬规则 R8：`strata-pixels`、`strata-core`、`strata-color`、`strata-io` 不得出现 `target_os = "macos"`，也不得依赖任何平台专属 crate；平台代码只允许出现在 `strata-render` 与 `strata-ffi`，且用 `cfg` 隔离。
- 文档像素约定：`PixelBuffer::Rgba8(Vec<u8>)` 或 `Rgba16(Vec<u16>)`，直通 alpha、行优先、左上原点；16 位为 0–65535 整数。尺寸乘法使用 checked arithmetic，明确样本数与字节数，解码有分配上限，不能先分配再检查。
- M0 文档保留原始位深、像素与源 ICC；Display P3（sRGB 传输曲线）仅是显示副本的空间。16 位转换走 16 位通道，显示纹理可用半精度浮点，但不得回写文档。M1 才加入可编辑工作空间与保存。
- M0 的 `set_document/open_image` 是命令系统前的引导接口；M1 必须迁入命令执行器并收窄可见性，不保留可绕过命令的正式修改路径。
- 视图坐标约定：全部使用**物理像素**（point × `backingScaleFactor`），原点在视口左上角，y 向下。
- 画布背景色：sRGB `#141415`（与界面令牌 `bg-canvas` 深色值一致）。
- 不使用 Adobe 的商标、图标或界面素材。
- 提交信息用 Conventional Commits（`feat:` `fix:` `chore:` `docs:` `ci:` `test:`）；署名按实际贡献者和仓库约定填写，不预置未参与者。
- 本计划不创建 `strata-ops`、`strata-ai`、`strata-ml`、`strata-devtool`：它们在第一次被需要的里程碑里创建（YAGNI）。

## Review Focus

1. **打开损坏或不支持的文件**（如把 `.txt` 改名为 `.jpg`、M0 尚不支持的 HEIC）：应弹出说明原因的提示，应用不崩溃。→ Task 3 `garbage_bytes_are_unsupported`、Task 6 `garbage_file_returns_unsupported_format`、Task 7 的错误弹窗（Task 8 手动验证第 6 项）。
2. **窗口被拖到极小或最小化导致视口宽 / 高为 0**：wgpu 对 0 尺寸表面配置会直接 panic，应跳过渲染。→ Task 5 `zero_size_resize_skips_rendering`、Task 8 `resize_canvas_to_zero_is_safe`。
3. **窗口在 Retina 与非 Retina 屏幕之间移动**：图片视觉大小和位置不应跳变，也不应变糊。→ Task 4 `rescaled_doubles_zoom_for_retina`、Task 5 `resize_with_new_scale_rescales_view`。
4. **图片内嵌的 ICC 配置文件损坏**：应退回按 sRGB 处理并标记，而不是打不开。→ Task 2 `invalid_icc_falls_back_to_srgb`。
5. **超过 GPU 最大纹理尺寸的图片**（如 20000 像素宽的全景图；M1 才有分块）：应返回明确的错误并提示，不崩溃。→ Task 4 `texture_size_check`、Task 5 `too_large_image_is_rejected`。
6. **16 位图片不能静默降成 8 位**：使用含相邻 16 位码值的渐变，不用纯白样例掩盖精度损失。→ Task 1b、Task 2/3/6 的 16 位测试。
7. **打开失败不能替换旧文档**：解码、ICC、GPU 准备失败均不得发出成功事件或改变旧画面；事件在提交并解锁后投递。→ Task 6 故障注入与回调重入测试。
8. **干净构建与画布释放**：无 Generated/工程/target 的 CI 能执行 `make test`；窗口关闭后停止 display link、detach 完成才释放 layer。→ Task 7–9。

---

## 文件结构

```
strata/
├── Cargo.toml                       workspace 声明与统一依赖
├── rust-toolchain.toml              固定工具链 1.97.1 + rustfmt + clippy
├── rustfmt.toml
├── Makefile                         make build / run / test / lint
├── LICENSE  NOTICE  README.md  CONTRIBUTING.md  CODE_OF_CONDUCT.md  .gitignore
├── .github/
│   ├── ISSUE_TEMPLATE/bug_report.yml, feature_request.yml
│   ├── pull_request_template.md
│   └── workflows/ci.yml
├── scripts/
│   ├── build-rust.sh                编译 strata-ffi 并生成 Swift 绑定
│   ├── check-portability.sh         R8 检查
│   ├── make-perf-image.sh           生成 2400 万像素测试图
│   └── make-p3-sample.sh            生成带 Display P3 配置文件的测试图
├── crates/
│   ├── strata-pixels/src/lib.rs     PixelBuffer、PixelFormat、尺寸/长度校验
│   ├── strata-color/src/lib.rs      Display P3 配置文件、转换、回退
│   ├── strata-io/src/lib.rs         保位深解码为 RGBA8/RGBA16 + ICC
│   ├── strata-io/tests/decode.rs
│   ├── strata-core/src/lib.rs       版本、事件、EventSink、ImageDocument、Engine
│   ├── strata-render/src/view.rs    Viewport、ViewTransform（纯数学）
│   ├── strata-render/src/renderer.rs  wgpu 渲染器（离屏 + macOS 表面）
│   ├── strata-render/src/shader.wgsl
│   ├── strata-render/src/lib.rs
│   ├── strata-render/tests/gpu.rs
│   └── strata-ffi/src/lib.rs        UniFFI 门面：StrataEngine、EngineListener、StrataError
├── tools/uniffi-bindgen/src/main.rs
├── apps/macos/
│   ├── project.yml                  XcodeGen 工程描述（.xcodeproj 不入库）
│   ├── Strata/App/AppDelegate.swift
│   ├── Strata/App/MainWindowController.swift
│   ├── Strata/Canvas/CanvasView.swift
│   ├── Strata/Canvas/FrameStatsHUD.swift
│   ├── Strata/Engine/EngineEvents.swift
│   ├── StrataTests/BridgeTests.swift
│   └── Generated/                   UniFFI 输出（不入库）
└── docs/research/                   三份技术验证结论（Task 10）
```

依赖方向：`strata-pixels` 无内部依赖；color、io、core、render → pixels；io 在测试中额外依赖 color；ffi → core、io、color、render、pixels。平台代码不得泄漏进共享像素类型。

---

### Task 1: 仓库地基与 workspace

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `.gitignore`, `LICENSE`, `NOTICE`, `README.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `.github/ISSUE_TEMPLATE/bug_report.yml`, `.github/ISSUE_TEMPLATE/feature_request.yml`, `.github/pull_request_template.md`
- Create: `crates/strata-core/Cargo.toml`, `crates/strata-core/src/lib.rs`

**Interfaces:**
- Produces: workspace 根；`strata_core::version() -> &'static str`

- [ ] **Step 0: 验证工具链前提**

读取本机 Rust/Xcode/Swift/XcodeGen 版本，核对候选依赖的实际发布版与 API；用最小构建验证 macOS 14 部署和 Linux 编译。失败时记录原因并同步修改工具链、workspace、README 与 CI，禁止凭文档中的版本号宣称验证通过。后续固定值均指此步确认后的版本。

- [x] **Step 1: 初始化 Git 仓库**

Run: `cd /Volumes/MoveMachine/Projects/Strata && git init -b main`
Expected: `Initialized empty Git repository`

- [x] **Step 2: 写 workspace 与工具链文件**

`Cargo.toml`：`[workspace] resolver = "3"`，初始 `members = ["crates/strata-core"]`，每次创建 crate 时显式添加（避免尚不存在的 `tools/*` 使第一条测试失败）；`[workspace.package]` 设 `version = "0.0.1"`、`edition = "2024"`、`license = "Apache-2.0"`、`rust-version = "1.97"`；`[workspace.dependencies]` 按 Global Constraints 声明依赖（`bytemuck` 开 `derive` 特性）。提交 `Cargo.lock`，应用构建/CI 使用锁定版本。
`rust-toolchain.toml`：`channel = "1.97.1"`，`components = ["rustfmt", "clippy"]`。
`rustfmt.toml`：`max_width = 100`。
`.gitignore`：`target/`、`build/`、`apps/macos/Generated/`、`apps/macos/*.xcodeproj/`、`.DS_Store`、`*.xcuserstate`、`xcuserdata/`。

- [x] **Step 3: 写 strata-core 的失败测试**

`crates/strata-core/src/lib.rs` 底部：

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn version_matches_cargo_package() {
        assert_eq!(super::version(), "0.0.1");
    }
}
```

- [x] **Step 4: 运行测试确认失败**

Run: `cargo test -p strata-core`
Expected: FAIL，报错 `cannot find function 'version'`

- [x] **Step 5: 实现 `pub fn version() -> &'static str`**

返回 `env!("CARGO_PKG_VERSION")`。crate 顶部写一行模块文档："Strata 文档模型、命令系统与事件（M0：仅版本与事件）"。

- [x] **Step 6: 运行测试确认通过**

Run: `cargo test -p strata-core`
Expected: PASS（1 passed）

- [x] **Step 7: 写开源规范文件**

- `LICENSE`：`curl -sL https://www.apache.org/licenses/LICENSE-2.0.txt -o LICENSE`，确认首行为 `Apache License`。
- `NOTICE`：`Strata` / `Copyright 2026 The Strata Authors`。
- `README.md`（中文为主，附一段英文简介）：一句话定位（照抄总体设计 §1）、截图 `docs/design/screenshots/ui-v1.png`、状态"M0 开发中，尚不可用"、运行要求 macOS 14+ 与 Step 0 实测的构建工具版本（区分运行最低系统与 Xcode 构建要求）、构建命令 `make run`、干净检出测试 `make test`、文档入口 `docs/README.md`、许可证。
- `CONTRIBUTING.md`：开发环境、`make` 各目标、8 条硬规则一句话摘要（链接总体设计 §4）、提交规范（Conventional Commits）、"M1 起新增功能必须是带说明书的命令"。
- `CODE_OF_CONDUCT.md`：采用 Contributor Covenant 2.1 原文（https://www.contributor-covenant.org/version/2/1/code_of_conduct/），执行联系方式写"通过仓库维护者的 GitHub 主页联系"，并在 Task 11 交付时提醒发起人是否改为邮箱。
- Issue 模板：bug（复现步骤、期望、实际、系统版本、Strata 版本、是否接入模型及模型名）；feature（场景、期望行为、"AI 应该如何调用它"）。
- PR 模板勾选项：测试通过；`make lint` 通过；未破坏硬规则 R1–R8；文档已更新。

- [x] **Step 8: 提交**

```bash
git add -A
git commit -m "chore: initialize repository, workspace and open-source docs"
```

---

### Task 1b: strata-pixels — 共享位深与缓冲契约

**Files:** Create `crates/strata-pixels/Cargo.toml`、`crates/strata-pixels/src/lib.rs`，更新 workspace members。

**Interfaces:**
- `PixelFormat { Rgba8, Rgba16 }` 与 `PixelBuffer { Rgba8(Vec<u8>), Rgba16(Vec<u16>) }`，派生 `Clone, Debug, PartialEq`。
- `format()`、`sample_len()`、`byte_len()`、`validate(width, height)`；`validate` 拒绝零尺寸、乘法溢出、样本数不符。错误区分尺寸溢出与长度不匹配，不用不安全 cast 把 `u8` 解释为 `u16`。

- [x] 先写并运行失败测试：8/16 位格式与字节数、相邻 16 位值保留、零尺寸、`u32::MAX` 尺寸乘法溢出、长度不匹配。
- [x] 实现共享类型；`cargo test -p strata-pixels` 全通过；不能出现平台/GPU 依赖。
- [x] 提交：`feat(pixels): define validated 8-bit and 16-bit pixel buffers`。

---

### Task 2: strata-color — 保位深转换到 Display P3

**Files:**
- Create: `crates/strata-color/Cargo.toml`, `crates/strata-color/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub fn display_p3_profile() -> lcms2::Profile`
  - `pub struct Converted { pub pixels: PixelBuffer, pub source_profile: String, pub profile_fallback: bool }`
  - `pub fn to_display_p3(pixels: &PixelBuffer, width: u32, height: u32, source_icc: Option<&[u8]>) -> Result<Converted, ColorError>`
  - `pub enum ColorError { BufferSize { expected: usize, actual: usize }, InvalidDimensions, UnsupportedProfile(String), Transform(String) }`（thiserror；BufferSize 单位为通道样本数）

- [x] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &PixelBuffer, b: &[u8], tol: i16) -> bool {
        let PixelBuffer::Rgba8(a) = a else { return false; };
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (*x as i16 - *y as i16).abs() <= tol)
    }

    #[test]
    fn srgb_red_maps_to_p3_234_51_35() {
        let c = to_display_p3(&PixelBuffer::Rgba8(vec![255, 0, 0, 255]), 1, 1, None).unwrap();
        assert!(close(&c.pixels, &[234, 51, 35, 255], 1), "{:?}", c.pixels);
        assert_eq!(c.source_profile, "sRGB (assumed)");
        assert!(!c.profile_fallback);
    }

    #[test]
    fn srgb_white_and_black_are_unchanged() {
        let c = to_display_p3(&PixelBuffer::Rgba8(vec![255, 255, 255, 255, 0, 0, 0, 255]), 2, 1, None).unwrap();
        assert!(close(&c.pixels, &[255, 255, 255, 255, 0, 0, 0, 255], 1));
    }

    #[test]
    fn p3_tagged_input_is_identity() {
        let icc = display_p3_profile().icc().unwrap();
        let c = to_display_p3(&PixelBuffer::Rgba8(vec![255, 0, 0, 255]), 1, 1, Some(&icc)).unwrap();
        assert!(close(&c.pixels, &[255, 0, 0, 255], 1));
        assert!(c.source_profile.contains("Display P3"));
    }

    #[test]
    fn invalid_icc_falls_back_to_srgb() {
        let c = to_display_p3(&PixelBuffer::Rgba8(vec![255, 0, 0, 255]), 1, 1, Some(b"not an icc profile")).unwrap();
        assert!(c.profile_fallback);
        assert_eq!(c.source_profile, "sRGB (fallback)");
        assert!(close(&c.pixels, &[234, 51, 35, 255], 1));
    }

    #[test]
    fn alpha_is_preserved() {
        let c = to_display_p3(&PixelBuffer::Rgba8(vec![255, 0, 0, 128]), 1, 1, None).unwrap();
        assert!(matches!(c.pixels, PixelBuffer::Rgba8(ref p) if p[3] == 128));
    }

    #[test]
    fn buffer_size_mismatch_is_error() {
        let e = to_display_p3(&PixelBuffer::Rgba8(vec![0; 7]), 1, 2, None).unwrap_err();
        assert!(matches!(e, ColorError::BufferSize { expected: 8, actual: 7 }));
    }
}
```

- [x] 另加 `rgba16_identity_preserves_adjacent_values`（P3→P3 的完整 16 位灰阶坡度、相邻码值可区分）、`rgba16_alpha_is_exact`、`rgba16_transform_matches_float_reference`（sRGB→P3 与浮点参考量化后每通道误差 ≤ 2/65535）、`non_rgb_icc_is_reported`。不可只用 0/65535 端点验收。

- [x] **Step 2: 运行测试确认失败**

Run: `cargo test -p strata-color`
Expected: FAIL，`cannot find function 'to_display_p3'`

- [x] **Step 3: 实现 `display_p3_profile()`**

用 lcms2 构造 RGB 矩阵配置文件，数值固定如下：白点 D65 `(0.3127, 0.3290)`；原色 R `(0.680, 0.320)`、G `(0.265, 0.690)`、B `(0.150, 0.060)`；三通道使用 sRGB 参数化曲线（ICC 类型 4，参数 `[2.4, 1/1.055, 0.055/1.055, 1/12.92, 0.04045]`）；描述标签设为 `Display P3`。

- [x] **Step 4: 实现 `to_display_p3`**

先用共享校验器检查尺寸和样本数。源配置文件：`None` → `Profile::new_srgb()`、`source_profile = "sRGB (assumed)"`；解析失败 → sRGB 回退并标记；能解析但与 RGBA 不兼容的 CMYK/灰度等 profile → `UnsupportedProfile`，不得直接交给 RGB transform 造成错误颜色。转换按 `PixelBuffer` 分派 `TYPE_RGBA_8 → TYPE_RGBA_8` 或 `TYPE_RGBA_16 → TYPE_RGBA_16`，相对色度意图，alpha 逐样本原样拷贝；结果位深与输入相同。16 位参考测试不通过时用浮点转换后一次量化，不允许先降到 8 位。灰度/CMYK ICC 的完整解码匹配在 M1 补齐，M0 明确报不支持。

- [x] **Step 5: 运行测试确认通过**

Run: `cargo test -p strata-color`
Expected: 全部通过（含 6 个 8 位/错误测试与新增 16 位/配置文件测试）。

- [x] **Step 6: 提交**

```bash
git add crates/strata-color
git commit -m "feat(color): convert images to Display P3 with sRGB fallback"
```

---

### Task 3: strata-io — 解码图片

**Files:**
- Create: `crates/strata-io/Cargo.toml`, `crates/strata-io/src/lib.rs`, `crates/strata-io/tests/decode.rs`

**Interfaces:**
- Consumes: `strata_pixels::PixelBuffer`；测试中使用 `strata_color::display_p3_profile()`（dev-dependency）
- Produces:
  - `pub struct DecodedImage { pub width: u32, pub height: u32, pub pixels: PixelBuffer, pub icc_profile: Option<Vec<u8>> }`
  - `pub fn decode_image(path: &Path) -> Result<DecodedImage, IoError>`
  - `pub enum IoError { NotFound(PathBuf), Unsupported { path: PathBuf, reason: String }, Decode { path: PathBuf, reason: String }, Io(#[from] std::io::Error) }`

- [x] **Step 1: 写失败测试**

`tests/decode.rs` 中用 `image` 的编码器在临时目录生成测试文件（dev-dependency：`tempfile`、`strata-color`）。测试与断言：

```rust
#[test] fn decodes_png_dimensions_and_pixels()
// 3×2 纯红 RGBA PNG → width 3, height 2, pixels 为 Rgba8，样本数 24，前 4 个样本 == [255, 0, 0, 255]

#[test] fn extracts_embedded_icc_profile()
// 用 PngEncoder 的 set_icc_profile 写入 display_p3_profile().icc() → icc_profile == Some(同一字节串)

#[test] fn untagged_png_has_no_profile()
// → icc_profile.is_none()

#[test] fn grayscale_16bit_png_preserves_precision()
// 2×2 无 ICC 的 L16 [0, 32768, 32769, 65535] → Rgba16；RGB 各通道保留对应值，alpha 65535

#[test] fn rgba16_tiff_preserves_samples()
// 带 alpha 的非端点/非 257 倍数 16 位样本，解码后逐样本一致

#[test] fn jpeg_decodes()
// 8×8 灰度 128 的 JPEG（质量 95）→ 每个通道与 128 相差 ≤ 3，alpha == 255

#[test] fn transparent_pixels_keep_alpha()
// 1×1 [10, 20, 30, 0] PNG → pixels == PixelBuffer::Rgba8(vec![10, 20, 30, 0])

#[test] fn garbage_bytes_are_unsupported()
// 内容为 b"hello" 的 photo.jpg → Err(IoError::Unsupported { .. })

#[test] fn missing_file_is_not_found()
// → Err(IoError::NotFound(p)) 且 p 等于传入路径
```

- [x] **Step 2: 运行测试确认失败**

Run: `cargo test -p strata-io`
Expected: FAIL，`cannot find function 'decode_image'`

- [x] **Step 3: 实现 `decode_image`**

用 `ImageReader::open(path)?.with_guessed_format()` 按内容识别格式；将打开时的 `NotFound` 映射为同名错误，不依赖有竞态的 exists 预检。解码前设置内存/尺寸上限（M0 解码缓冲上限 512 MiB），先读 ICC，再按实际位深分派：8 位转 `Rgba8`、16 位转 `Rgba16`；灰度展开仍保留位深，浮点/HDR 或不支持模式明确拒绝。使用 checked arithmetic 验证总大小，超限在分配前报错；补超限/溢出测试。损坏 ICC 的回退状态由色彩层处理，不静默丢弃。

- [x] **Step 4: 运行测试确认通过**

Run: `cargo test -p strata-io`
Expected: 全部通过（含保位深 PNG/TIFF 与分配上限测试）。

- [x] **Step 5: 提交**

```bash
git add crates/strata-io
git commit -m "feat(io): decode 8-bit and 16-bit images without precision loss"
```

---

### Task 4: strata-core 事件 + strata-render 视图数学

两部分都是无 GPU、无 IO 的纯逻辑，放在同一个任务里。

**Files:**
- Modify: `crates/strata-core/src/lib.rs`
- Create: `crates/strata-render/Cargo.toml`, `crates/strata-render/src/lib.rs`, `crates/strata-render/src/view.rs`

**Interfaces:**
- Produces（core）：
  - `#[derive(Clone, Debug, PartialEq)] pub struct DocumentInfo { pub width: u32, pub height: u32, pub bit_depth: u8, pub source_profile: String, pub profile_fallback: bool }`
  - `#[derive(Clone, Debug, PartialEq)] pub enum EngineEvent { DocumentOpened(DocumentInfo) }`
  - `pub trait EventSink: Send + Sync { fn emit(&self, event: EngineEvent); }`
  - `pub struct ImageDocument { pub info: DocumentInfo, pub pixels: PixelBuffer, pub icc_profile: Option<Vec<u8>> }`：保留源像素，不存屏幕量化副本
  - `pub struct Engine`：`pub fn new() -> Self`、`pub fn set_document(&mut self, doc: ImageDocument) -> EngineEvent`、`pub fn document(&self) -> Option<&ImageDocument>`。事件作为值返回给 ffi，在所有状态提交并解锁后再由 EventSink 投递
- Produces（render::view，从 `strata_render` 根重新导出）：
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct Viewport { pub width: u32, pub height: u32, pub scale: f32 }`
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct ViewTransform { pub zoom: f32, pub center_x: f32, pub center_y: f32 }`：zoom = 每个图像像素占多少物理屏幕像素；center = 视口中心处的图像坐标
  - `pub const MIN_ZOOM: f32 = 0.01; pub const MAX_ZOOM: f32 = 64.0; pub const FIT_MARGIN: f32 = 0.9;`
  - `ViewTransform::fit(image_w: u32, image_h: u32, vp: Viewport) -> Self`
  - `ViewTransform::screen_to_image(&self, vp: Viewport, sx: f32, sy: f32) -> (f32, f32)`
  - `ViewTransform::image_to_screen(&self, vp: Viewport, ix: f32, iy: f32) -> (f32, f32)`
  - `ViewTransform::pan_by(&mut self, dx: f32, dy: f32)`：让内容在屏幕上移动 (dx, dy) 物理像素
  - `ViewTransform::zoom_at(&mut self, vp: Viewport, sx: f32, sy: f32, factor: f32)`
  - `ViewTransform::rescaled(&self, old_scale: f32, new_scale: f32) -> Self`
  - `ViewTransform::image_to_ndc(&self, vp: Viewport, image_w: u32, image_h: u32) -> [[f32; 4]; 4]`：列主序，把单位四边形 (0..1, 0..1) 映射到 NDC（y 向上）
  - `pub fn check_texture_fits(width: u32, height: u32, max_dim: u32) -> Result<(), RenderError>`
  - `pub enum RenderError { NoAdapter, RequestDevice(String), Surface(String), ImageTooLarge { width: u32, height: u32, max: u32 }, BufferSize { expected: usize, actual: usize }, NotOffscreen }`（thiserror，放在 `lib.rs`）

- [x] **Step 1: 写 core 的失败测试**

```rust
#[derive(Default)]
struct Recorder(std::sync::Mutex<Vec<EngineEvent>>);
impl EventSink for Recorder {
    fn emit(&self, e: EngineEvent) { self.0.lock().unwrap().push(e); }
}

fn doc(w: u32, h: u32) -> ImageDocument {
    ImageDocument {
        info: DocumentInfo { width: w, height: h, bit_depth: 8, source_profile: "sRGB (assumed)".into(), profile_fallback: false },
        pixels: PixelBuffer::Rgba8(vec![0; (w * h * 4) as usize]),
        icc_profile: None,
    }
}

#[test]
fn set_document_returns_committed_event() {
    let rec = Arc::new(Recorder::default());
    let mut engine = Engine::new();
    let event = engine.set_document(doc(4, 3));
    assert_eq!(engine.document().unwrap().info.width, 4);
    assert!(rec.0.lock().unwrap().is_empty()); // core 不直接回调
    rec.emit(event.clone()); // 模拟宿主提交后投递
    assert_eq!(*rec.0.lock().unwrap(), vec![event]);
}

#[test]
fn second_document_replaces_first() {
    let mut engine = Engine::new();
    engine.set_document(doc(4, 3));
    engine.set_document(doc(8, 6));
    assert_eq!(engine.document().unwrap().info.width, 8);
}
```

- [x] **Step 2: 写 view 的失败测试**（`view.rs` 内 `#[cfg(test)]`，浮点用 `assert!((a - b).abs() < 1e-3)`）

```rust
const VP: Viewport = Viewport { width: 800, height: 600, scale: 1.0 };

#[test] fn fit_centers_and_leaves_margin()
// fit(1000, 2000, Viewport{1000,1000,1}) → zoom 0.45, center (500, 1000)

#[test] fn screen_image_roundtrip()
// t = {zoom 2.5, center (123, 456)}；image_to_screen(screen_to_image(37, 81)) == (37, 81)

#[test] fn zoom_at_keeps_point_under_cursor_fixed()
// t = {zoom 1, center (400, 300)}；zoom_at(VP, 100, 100, 2.0) 后 zoom == 2，screen_to_image(VP, 100, 100) == (100, 100)

#[test] fn zoom_is_clamped()
// zoom_at(.., 1e6) → zoom == MAX_ZOOM；再 zoom_at(.., 1e-9) → zoom == MIN_ZOOM

#[test] fn pan_moves_content_with_fingers()
// t = {zoom 2, center (400, 300)}；p = screen_to_image(VP, 400, 300)；pan_by(20, 10)；image_to_screen(VP, p) == (420, 310)

#[test] fn pan_is_unbounded_for_infinite_canvas()
// t = {zoom 1, center (400, 300)}；pan_by(1e6, -1e6) → center == (400 - 1e6, 300 + 1e6)，且都是有限值

#[test] fn rescaled_doubles_zoom_for_retina()
// {zoom 0.5, center (10, 20)}.rescaled(1.0, 2.0) == {zoom 1.0, center (10, 20)}

#[test] fn image_to_ndc_maps_center_to_origin_and_y_up()
// t = {zoom 1, center (400, 300)}，图像 800×600：单位坐标 (0.5, 0.5) → NDC (0, 0)；(1.0, 0.5) → (1, 0)；(0.5, 0.0) → (0, 1)

#[test] fn texture_size_check()
// check_texture_fits(16384, 100, 16384) → Ok；(16385, 100, 16384) → Err(ImageTooLarge { width: 16385, height: 100, max: 16384 })
```

- [x] **Step 3: 运行测试确认失败**

Run: `cargo test -p strata-core -p strata-render`
Expected: FAIL，报错缺少 `Engine`、`ViewTransform` 等定义

- [x] **Step 4: 实现 core 事件与 Engine**

`set_document` 只接收已验证的候选文档，保存后返回 `DocumentOpened(info.clone())`；不在方法内调用外部 sink。ffi 负责锁外投递，避免回调重入死锁。

- [x] **Step 5: 实现 view.rs**

公式（视口中心 `vc = (vp.width/2, vp.height/2)`）：
- `screen_to_image`：`(cx + (sx - vc.x) / zoom, cy + (sy - vc.y) / zoom)`
- `fit`：`zoom = min(vw/iw, vh/ih) * FIT_MARGIN`，center = 图像中心
- `zoom_at`：先求 `p = screen_to_image(s)`，`zoom = clamp(zoom * factor, MIN_ZOOM, MAX_ZOOM)`，再令 `center = p - (s - vc) / zoom`
- `pan_by`：`center -= (dx / zoom, dy / zoom)`，不做任何边界限制（无限画布）
- `rescaled`：`zoom *= new_scale / old_scale`，center 不变

- [x] **Step 6: 运行测试确认通过**

Run: `cargo test -p strata-core -p strata-render`
Expected: PASS（core 3 passed，render 9 passed）

- [x] **Step 7: 提交**

```bash
git add crates/strata-core crates/strata-render
git commit -m "feat(core,render): engine events and infinite-canvas view transform"
```

---

### Task 5: strata-render — wgpu 渲染器（离屏）

**Files:**
- Create: `crates/strata-render/src/renderer.rs`, `crates/strata-render/src/shader.wgsl`, `crates/strata-render/tests/gpu.rs`
- Modify: `crates/strata-render/src/lib.rs`（导出 `Renderer`、`PreparedImage`、`FrameStats`、`CANVAS_BG_SRGB`）

**Interfaces:**
- Consumes: Task 4 的 `Viewport`、`ViewTransform`、`check_texture_fits`、`RenderError`
- Produces:
  - `pub const CANVAS_BG_SRGB: [u8; 3] = [0x14, 0x14, 0x15];`
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct FrameStats { pub cpu_ms: f32, pub skipped: bool }`
  - `pub struct Renderer`，方法：
    - `pub fn new_offscreen(vp: Viewport) -> Result<Self, RenderError>`
    - `pub fn prepare_image(&self, width: u32, height: u32, pixels_p3: &PixelBuffer) -> Result<PreparedImage, RenderError>`：校验并准备新纹理/fit 参数，不改变当前图像或视图
    - `pub fn commit_image(&mut self, prepared: PreparedImage)`：只交换已准备好的资源，不再做可失败的分配/转换；PreparedImage 绑定创建它的 renderer，禁止跨 renderer 提交
    - `pub fn view(&self) -> ViewTransform` / `pub fn set_view(&mut self, v: ViewTransform)`
    - `pub fn viewport(&self) -> Viewport`
    - `pub fn resize(&mut self, vp: Viewport)`：宽或高为 0 时只记录、不重配目标；scale 变化时视图按 `rescaled(old, new)` 调整
    - `pub fn render(&mut self) -> Result<FrameStats, RenderError>`：视口为 0 时返回 `skipped: true`
    - `pub fn max_texture_dimension(&self) -> u32`
    - `pub fn read_offscreen_rgba(&self) -> Result<Vec<u8>, RenderError>`：读回 8 位显示结果（P3 原色、sRGB 传输曲线），仅作显示测试；不得用于文档保存或 16 位精度验收，表面模式返回 `NotOffscreen`

- [x] **Step 1: 写失败测试**

`tests/gpu.rs` 顶部提供 `fn renderer_or_skip(vp) -> Option<Renderer>`：`new_offscreen` 返回 `Err(RenderError::NoAdapter)` 时打印 `skipped: no GPU adapter` 并返回 `None`（Linux CI 没有 GPU）。像素比较允许 ±2。

正常图像测试先 `prepare_image` 再 `commit_image`；8 位样本用 `PixelBuffer::Rgba8`。设置 `STRATA_REQUIRE_GPU=1` 时无 adapter 必须失败；本机 M0 GPU 验收使用该开关，CI 跳过只能说明核心测试通过。

```rust
#[test] fn renders_image_center_pixel()
// vp 64×64；图像 2×2 全 [200, 100, 50, 255]；set_view({zoom 16, center (1, 1)})；render；像素 (32, 32) ≈ [200, 100, 50, 255]

#[test] fn outside_image_shows_canvas_background()
// 同上设置；像素 (0, 0) ≈ [0x14, 0x14, 0x15, 255]

#[test] fn transparent_image_shows_background()
// 图像 2×2 全 [255, 0, 0, 0]；像素 (32, 32) ≈ 背景色

#[test] fn render_without_image_shows_background()
// 不提交任何图像；像素 (10, 10) ≈ 背景色

#[test] fn zero_size_resize_skips_rendering()
// resize(Viewport { width: 0, height: 64, scale: 1.0 })；render() == Ok(FrameStats { skipped: true, .. })，且不 panic

#[test] fn too_large_image_is_rejected()
// w = max_texture_dimension() + 1，h = 1；prepare_image → Err(ImageTooLarge { width: w, height: 1, .. })，旧图/视图不变

#[test] fn resize_with_new_scale_rescales_view()
// vp 64×64 scale 1；set_view({zoom 1, center (5, 5)})；resize({128, 128, 2.0})；view() == {zoom 2, center (5, 5)}

#[test] fn failed_prepare_preserves_image_and_view()
// 已提交 A 后，注入 B 的分配/验证失败；A 的离屏像素与 view 均不变

#[test] fn rgba16_preview_does_not_mutate_source()
// 输入含相邻码值的 Rgba16；准备/渲染后原缓冲逐样本不变，预览有正确的亮度渐变

#[test] fn transparent_edge_has_no_color_fringe()
// 透明彩色像素与不透明像素交界，缩放过滤后应符合先线性化/预乘再插值的参考
```

- [x] **Step 2: 运行测试确认失败**

Run: `cargo test -p strata-render --test gpu`
Expected: FAIL，`cannot find struct 'Renderer'`

- [x] **Step 3: 实现 Renderer 与着色器**

- 渲染目标抽象为私有枚举 `Target { Offscreen(wgpu::Texture), Surface { .. } }`（Surface 在 Task 8 填充）。离屏纹理格式 `Rgba8UnormSrgb`。
- 显示缓存：按 8/16 位原始值归一化，用 f32 解码 sRGB 传输曲线并预乘 alpha，再转 `Rgba16Float` 纹理（需要半精度编码依赖时在 workspace 声明并锁定）。先预乘后过滤，避免透明边缘色晕；M0 不生成 mipmap，M1 分块时再做。该缓存只用于显示，原始 `PixelBuffer` 不变。
- 着色器：用 uniform 里的 `image_to_ndc` 矩阵变换单位四边形，采样得到线性 P3 预乘颜色；使用预乘 alpha 混合。sRGB 格式目标只负责传输曲线编码，layer 的 P3 colorspace 决定原色，不能再重复 gamma 转换。
- 清屏色：把 `CANVAS_BG_SRGB` 转为线性值后作为 `wgpu::Color`（因为目标是 sRGB 格式）。
- `prepare_image` 先检查尺寸/样本数/纹理上限，再准备临时纹理；验证错误作用域与资源失败映射为 `RenderError`，成功才返回 PreparedImage。`commit_image` 仅交换资源并设置 fit；准备失败、候选被取消时销毁候选，旧资源保留。
- 适配器请求用 `pollster::block_on`；`cpu_ms` 用 `std::time::Instant` 计量 `render` 的耗时。

- [x] **Step 4: 运行测试确认通过**

Run: `cargo test -p strata-render`
Expected: 全部通过（原 7 个 GPU 场景 + 新增失败原子性/位深/透明边缘场景 + 9 个 view 测试）。本机设置 `STRATA_REQUIRE_GPU=1`，不能靠跳过达标。

- [x] **Step 5: 提交**

```bash
git add crates/strata-render
git commit -m "feat(render): offscreen wgpu renderer with view transform and canvas background"
```

---

### Task 6: strata-ffi — UniFFI 门面与 Swift 绑定生成

**Files:**
- Create: `crates/strata-ffi/Cargo.toml`（`crate-type = ["lib", "staticlib", "cdylib"]`）、`crates/strata-ffi/src/lib.rs`
- Create: `tools/uniffi-bindgen/Cargo.toml`、`tools/uniffi-bindgen/src/main.rs`（调用 `uniffi::uniffi_bindgen_main()`，依赖开启 `uniffi` 的 `cli` 特性）
- Create: `scripts/build-rust.sh`

**Interfaces:**
- Consumes: `strata_io::decode_image`、`strata_color::to_display_p3`、`strata_core::{Engine, EventSink, EngineEvent, ImageDocument, DocumentInfo, version}`、`strata_render::{Renderer, Viewport}`
- Produces（全部 UniFFI 导出，Swift 侧名称由 UniFFI 自动转为驼峰）：
  - Record 与 Enum 一律同时派生 `Debug, Clone, PartialEq`（测试需要比较）
  - `#[derive(uniffi::Record)] pub struct ImageInfo { pub width: u32, pub height: u32, pub bit_depth: u8, pub source_profile: String, pub profile_fallback: bool }`
  - `#[derive(uniffi::Record)] pub struct CanvasViewport { pub width: u32, pub height: u32, pub scale: f32 }`
  - `#[derive(uniffi::Record)] pub struct FrameInfo { pub cpu_ms: f32, pub skipped: bool }`
  - `#[derive(uniffi::Enum)] pub enum EngineEventDto { DocumentOpened { info: ImageInfo, generation: u64 } }`
  - `#[derive(Debug, thiserror::Error, uniffi::Error)] pub enum StrataError { NotFound { path: String }, UnsupportedFormat { message: String }, Decode { message: String }, Color { message: String }, Render { message: String }, Cancelled { message: String }, NoCanvas }`
  - `#[uniffi::export(with_foreign)] pub trait EngineListener: Send + Sync { fn on_event(&self, event: EngineEventDto); }`
  - `#[derive(uniffi::Object)] pub struct StrataEngine`（内部 `Mutex<Inner>`，`Inner { engine: strata_core::Engine, renderer: Option<Renderer>, open_generation: u64 }`；listener 在锁外持有），导出方法：
    - `#[uniffi::constructor] pub fn new(listener: Arc<dyn EngineListener>) -> Arc<Self>`
    - `pub fn core_version(&self) -> String`
    - `pub fn document_info(&self) -> Option<ImageInfo>`：持有状态锁读取已提交文档，用于 UI 与回调重入测试
    - `pub fn open_image(&self, path: String) -> Result<ImageInfo, StrataError>`
    - `pub fn render_frame(&self) -> Result<FrameInfo, StrataError>`：没有画布时返回 `NoCanvas`
  - 画布相关方法（`attach_canvas` 等）在 Task 8 加入。

- [x] **Step 1: 写失败测试**（`lib.rs` 内 `#[cfg(test)]`，用 `image` 在临时目录写 4×3 红色 PNG 作为测试文件）

```rust
#[derive(Default)]
struct Rec(std::sync::Mutex<Vec<EngineEventDto>>);
impl EngineListener for Rec {
    fn on_event(&self, e: EngineEventDto) { self.0.lock().unwrap().push(e); }
}

#[test] fn open_image_returns_info_and_notifies_listener()
// open_image(png) → Ok(ImageInfo { width: 4, height: 3, bit_depth: 8, source_profile: "sRGB (assumed)", profile_fallback: false })
// 监听器恰好收到 1 个 DocumentOpened，内含同样的 info

#[test] fn open_missing_file_returns_not_found()
// → Err(StrataError::NotFound { path }) 且 path 等于传入路径

#[test] fn garbage_file_returns_unsupported_format()
// 内容 b"hello" 的 x.jpg → Err(StrataError::UnsupportedFormat { .. })；监听器未收到任何事件

#[test] fn render_without_canvas_is_no_canvas_error()
// render_frame() → Err(StrataError::NoCanvas)

#[test] fn core_version_is_workspace_version()
// core_version() == "0.0.1"

#[test] fn failed_open_keeps_old_document_and_emits_no_success()
// 先开 A；分别注入 B 的解码、色彩、纹理尺寸/准备失败；A 的像素、信息、画面和视图不变，事件数不增加

#[test] fn listener_can_reenter_without_deadlock()
// DocumentOpened 回调中调用需要状态锁的 document_info；观察到已提交的 B，且调用及时返回

#[test] fn open_rgba16_preserves_source_samples()
// 16 位 PNG 原始样本与 ICC 在 core 中逐项保留，返回 bit_depth 16；显示转换不能覆盖它们

#[test] fn stale_open_cannot_replace_newer_request()
// A 的慢解码晚于 B 返回；A 的 generation 已失效，不替换 B、不发送 A 的成功事件
```

- [x] **Step 2: 运行测试确认失败**

Run: `cargo test -p strata-ffi`
Expected: FAIL，`cannot find struct 'StrataEngine'`

- [x] **Step 3: 实现 StrataEngine**

`open_image`：登记 generation → 锁外 `decode_image`（保留源像素/ICC）→ `to_display_p3`（生成独立显示副本）→ 在串行渲染/状态临界区检查 generation 并 `prepare_image` → 全部成功后一次提交 renderer 与 core 文档 → 解锁 → 投递 `DocumentOpened`。不得先 set_document 再做可能失败的 GPU 准备。没有 renderer 时提交源文档，随后 attach 若失败仅报画布错误，不丢文档。新请求淘汰旧 generation；失效请求返回 `Cancelled`，不弹普通打开失败警报。GPU 准备失败用可注入适配层测试，不依赖真的耗尽显存。

错误映射：`IoError::NotFound` → `NotFound`，`Unsupported` → `UnsupportedFormat`，`Decode`/`Io` → `Decode`，`ColorError` → `Color`，`RenderError` → `Render`。所有回调在锁外执行，事件带 generation 并按提交顺序投递，外壳忽略旧 generation；UniFFI DTO 同步携带该字段。crate 根部写 `uniffi::setup_scaffolding!();`。

- [x] **Step 4: 运行测试确认通过**

Run: `cargo test -p strata-ffi`
Expected: 全部通过（原 5 项 + 失败原子性、回调重入、16 位保真、过期请求测试）。

- [x] **Step 5: 实现 `scripts/build-rust.sh [Debug|Release]`**

参数不区分大小写，默认 Debug；Release 时传 `--release`。依次执行：
1. `cargo build -p strata-ffi --locked`（Release 时追加 `--release`）
2. `cargo run -q --locked -p uniffi-bindgen -- generate --library target/<profile>/libstrata_ffi.dylib --language swift --out-dir apps/macos/Generated`

脚本开头 `set -euo pipefail`，并 `cd` 到仓库根目录（以脚本自身位置为准），这样从 Xcode 调用也能找到路径。

- [x] **Step 6: 验证绑定生成**

Run: `chmod +x scripts/build-rust.sh && scripts/build-rust.sh && ls apps/macos/Generated`
Expected: 列出 `strata_ffi.swift`、`strata_ffiFFI.h`、`strata_ffiFFI.modulemap`

- [x] **Step 7: 提交**

```bash
git add crates/strata-ffi tools/uniffi-bindgen scripts/build-rust.sh
git commit -m "feat(ffi): UniFFI engine facade with listener callbacks and Swift binding generation"
```

---

### Task 7: macOS 应用外壳与桥接测试

**Files:**
- Create: `apps/macos/project.yml`, `apps/macos/Strata/App/AppDelegate.swift`, `apps/macos/Strata/App/MainWindowController.swift`, `apps/macos/Strata/Engine/EngineEvents.swift`, `apps/macos/StrataTests/BridgeTests.swift`, `Makefile`

**Interfaces:**
- Consumes: UniFFI 生成的 Swift API：`StrataEngine(listener:)`、`coreVersion()`、`openImage(path:)`、`EngineListener` 协议、`EngineEventDto.documentOpened(info:generation:)`、`StrataError`
- Produces:
  - `final class EngineEvents: EngineListener, @unchecked Sendable`，`init(onEvent: @escaping @MainActor (EngineEventDto) -> Void)`：`onEvent(event:)` 总是切到主线程后再调用闭包
  - `final class MainWindowController: NSWindowController`，`init(engine: StrataEngine)`，`func showDocumentInfo(_ info: ImageInfo)`：把窗口副标题设为 `"\(w) × \(h) · \(profile) · \(bitDepth) 位"`
  - `AppDelegate`（`@main`）持有 `engine: StrataEngine` 与 `windowController`

- [x] **Step 1: 写 XcodeGen 工程描述 `apps/macos/project.yml`**

固定值：工程名 `Strata`；部署目标 macOS `14.0`；`SWIFT_VERSION: 6.0`；Bundle ID `dev.strata-editor.Strata`（Task 11 可按 GitHub 组织调整）；应用 target `Strata`（`type: application`，源码 `Strata/` 与 `Generated/`）；资源引用 `../../docs/design/assets/sample-photo.jpg`，打包后命名为 `sample.jpg`；测试 target `StrataTests`（`bundle.unit-test`，宿主为 Strata）。构建设置：
- `LIBRARY_SEARCH_PATHS: $(SRCROOT)/../../target/debug`（Release 配置为 `target/release`）
- `OTHER_LDFLAGS` 显式链接 `$(SRCROOT)/../../target/<profile>/libstrata_ffi.a`；dylib 仅供生成绑定，避免 `-lstrata_ffi` 误选未打包的动态库导致运行时加载失败
- `OTHER_SWIFT_FLAGS: -Xcc -fmodule-map-file=$(SRCROOT)/Generated/strata_ffiFFI.modulemap`
- 链接系统框架 `Metal`、`QuartzCore`、`CoreGraphics`、`AppKit`
- `preBuildScripts`：`"$SRCROOT/../../scripts/build-rust.sh" "$CONFIGURATION"`
- Info.plist 由 XcodeGen 生成：`CFBundleDisplayName = Strata`、`NSPrincipalClass = NSApplication`、`LSMinimumSystemVersion = 14.0`

- [x] **Step 2: 写 `Makefile`**

目标与命令（`CONFIGURATION ?= Debug`，支持 `Release`；相关目标声明 `.PHONY`）：
- `rust` → `scripts/build-rust.sh $(CONFIGURATION)`
- `project` → `rust` 后执行 `cd apps/macos && xcodegen generate`（绑定文件必须在生成工程前存在）
- `build` → 依赖 `project`，执行 `xcodebuild -project apps/macos/Strata.xcodeproj -scheme Strata -configuration $(CONFIGURATION) -derivedDataPath build/DerivedData build`
- `run` → 依赖 `build`，执行 `open build/DerivedData/Build/Products/$(CONFIGURATION)/Strata.app`；`FRAME_STATS=1` 时追加 `--args --frame-stats`
- `test` → 必须依赖 `project`（后者先生成 Rust/Swift 绑定，再生成 Xcode 工程），然后 `cargo test --workspace --locked` 加同配置的 `xcodebuild … test`；干净检出不能依赖本机已生成的工程
- `lint` → `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`scripts/check-portability.sh`（脚本在 Task 9 创建，`lint` 目标在那时启用这一行）

- [x] **Step 3: 写失败的 XCTest `BridgeTests.swift`**

```swift
import XCTest
@testable import Strata

final class BridgeTests: XCTestCase {
    func testCoreVersionIsSemver() {
        let engine = StrataEngine(listener: EngineEvents { _ in })
        XCTAssertNotNil(engine.coreVersion().range(of: #"^\d+\.\d+\.\d+$"#, options: .regularExpression))
    }

    func testOpenImageDeliversEventOnMainThread() throws {
        let opened = expectation(description: "documentOpened")
        let events = EngineEvents { event in
            XCTAssertTrue(Thread.isMainThread)
            if case let .documentOpened(info, _) = event {
                XCTAssertEqual(info.width, 1024)
                XCTAssertEqual(info.height, 1536)
                XCTAssertEqual(info.bitDepth, 8)
                opened.fulfill()
            }
        }
        let engine = StrataEngine(listener: events)
        let url = try XCTUnwrap(Bundle.main.url(forResource: "sample", withExtension: "jpg"))
        DispatchQueue.global().async { _ = try? engine.openImage(path: url.path) }
        wait(for: [opened], timeout: 5)
    }

    func testOpenMissingFileThrowsNotFound() {
        let engine = StrataEngine(listener: EngineEvents { _ in })
        XCTAssertThrowsError(try engine.openImage(path: "/nonexistent/a.png")) { error in
            guard case StrataError.NotFound = error else { return XCTFail("\(error)") }
        }
    }
}
```

（示例照片的尺寸是 1024×1536，来自 `docs/design/assets/sample-photo.jpg`。UniFFI 生成的 Swift 错误枚举分支保留 Rust 的大小写，如 `StrataError.NotFound`；若生成代码不同，以生成代码为准调整测试，但不能改变断言含义。）

- [x] **Step 4: 运行测试确认失败**

Run: `make project && xcodebuild -project apps/macos/Strata.xcodeproj -scheme Strata -derivedDataPath build/DerivedData test`
Expected: FAIL，编译错误 `cannot find 'EngineEvents' in scope`

- [x] **Step 5: 实现 EngineEvents、AppDelegate、MainWindowController**

- `EngineEvents.onEvent`：`DispatchQueue.main.async { MainActor.assumeIsolated { onEvent(event) } }`。
- `AppDelegate`：启动时创建 `StrataEngine` 和窗口。菜单：应用菜单（关于、退出）、文件（打开… ⌘O，用 `NSOpenPanel`，允许的类型为 png、jpeg、webp、tiff、gif、bmp）、视图（适合屏幕 ⌘0、放大 ⌘=、缩小 ⌘-；Task 8 之前先不连接动作）。
- `MainWindowController`：窗口 1280×800、最小 960×600、`titlebarAppearsTransparent = false`、`toolbarStyle = .unified`，标题 `Strata`。
- `openImage` 在后台执行，主线程显示进度/错误；`NotFound` → “找不到文件”，`UnsupportedFormat` → “暂不支持这种图片格式”，其他 → “无法打开图片”并附详情；`Cancelled` 静默结束过期请求。成功事件按 generation 更新 UI，不能用旧事件覆盖新标题。
- 若生成的绑定在 Swift 6 严格并发模式下编译不过，把 `SWIFT_VERSION` 降为 `5.0`，并在 Task 10 的桥接结论里记录原因。

- [x] **Step 6: 运行测试确认通过**

Run: `xcodebuild -project apps/macos/Strata.xcodeproj -scheme Strata -derivedDataPath build/DerivedData test`
Expected: `** TEST SUCCEEDED **`（3 个测试通过）

- [x] **Step 7: 提交**

```bash
git add apps/macos/project.yml apps/macos/Strata apps/macos/StrataTests Makefile
git commit -m "feat(macos): app shell with UniFFI bridge and main-thread event delivery"
```

---

### Task 8: 画布 — wgpu 渲染到 CAMetalLayer、平移缩放、帧率显示

**Files:**
- Modify: `crates/strata-render/src/renderer.rs`（macOS 表面目标）
- Modify: `crates/strata-ffi/src/lib.rs`（画布方法）
- Create: `apps/macos/Strata/Canvas/CanvasView.swift`, `apps/macos/Strata/Canvas/FrameStatsHUD.swift`
- Modify: `apps/macos/Strata/App/MainWindowController.swift`、`AppDelegate.swift`（嵌入画布、连接视图菜单、启动时打开示例照片）

**Interfaces:**
- Consumes: Task 5 的 `Renderer`，Task 6 的 `StrataEngine`，Task 7 的 `EngineEvents`、`MainWindowController`
- Produces:
  - render：`#[cfg(target_os = "macos")] pub unsafe fn new_for_metal_layer(layer: *mut std::ffi::c_void, vp: Viewport) -> Result<Renderer, RenderError>`，用 `wgpu::SurfaceTargetUnsafe::CoreAnimationLayer`；表面格式优先 `Bgra8UnormSrgb`，呈现模式 `Fifo`
  - ffi `StrataEngine` 新增：
    - `pub fn attach_canvas(&self, metal_layer_ptr: u64, viewport: CanvasViewport) -> Result<(), StrataError>`：非 macOS 返回 `Render`；准备候选 renderer 与已有文档的显示副本，成功才替换旧 renderer
    - `pub fn detach_canvas(&self)`：同步停止该表面的使用并释放 renderer，幂等；文档仍保留
    - `pub fn resize_canvas(&self, viewport: CanvasViewport)`
    - `pub fn pan_by(&self, dx: f32, dy: f32)`
    - `pub fn zoom_at(&self, x: f32, y: f32, factor: f32)`
    - `pub fn fit_to_screen(&self)`
    - 没有画布时以上四个方法都是空操作
  - Swift：`final class CanvasView: NSView`，`init(engine: StrataEngine)`，`func markDirty()`

- [ ] **Step 1: 写 ffi 的失败测试**

```rust
#[test] fn resize_canvas_to_zero_is_safe()
// 未连接画布：resize_canvas(CanvasViewport { width: 0, height: 0, scale: 2.0 }) 不 panic

#[test] fn pan_zoom_fit_without_canvas_are_noops()
// pan_by(10, 10)、zoom_at(0, 0, 2.0)、fit_to_screen() 不 panic；render_frame() 仍为 Err(NoCanvas)

#[test] fn detach_is_idempotent_and_preserves_document()
// attach → detach 两次 → render_frame 返回 NoCanvas，文档像素不变；重连可恢复
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p strata-ffi`
Expected: FAIL，`no method named 'resize_canvas'`

- [x] **Step 3: 实现 render 的 macOS 表面与 ffi 画布方法**

`attach_canvas` 把 `u64` 转回 `*mut c_void` 后调用 `new_for_metal_layer`；该入口只给可信外壳，不注册给 AI。`unsafe` 前提：layer 在 attach 成功到同步 detach 完成期间始终由 Swift 强持有，渲染/resize/attach/detach 串行执行；detach 等待在途表面使用结束。指针非零不等于有效，不能让任意整数进入公开命令。表面 resize 宽或高为 0 时跳过配置；处理表面过期/丢失，不直接 panic。

- [x] **Step 4: 运行 Rust 测试确认通过**

Run: `cargo test --workspace`
Expected: 全部 PASS

- [x] **Step 5: 实现 CanvasView**

- `wantsLayer = true`；`makeBackingLayer()` 返回 `CAMetalLayer`，`colorspace = CGColorSpace(name: CGColorSpace.displayP3)`，`contentsScale = window.backingScaleFactor`。
- `viewDidMoveToWindow`：有 window 时以 `UInt64(UInt(bitPattern: Unmanaged.passUnretained(metalLayer).toOpaque()))` 调用 `attachCanvas`；离开 window 或关闭时先停止/失效 display link，再同步 detach，最后才释放 layer；重复 attach 前也按该顺序清理。视口按物理像素传入。
- `setFrameSize` 与 `viewDidChangeBackingProperties`：更新 `contentsScale`，调用 `resizeCanvas`（`drawableSize` 由 wgpu 配置表面时设置），然后 `markDirty()`。
- 帧循环：`displayLink(target:selector:)`（macOS 14 API）；每次回调只在 dirty 时调用 `renderFrame()` 并清除 dirty。
- 输入（所有坐标换算为物理像素、原点左上）：
  - `scrollWheel`：`panBy(dx: scrollingDeltaX * scale, dy: scrollingDeltaY * scale)`，内容跟随手指
  - `magnify(with:)`：以光标位置调用 `zoomAt(x:y:factor: 1 + magnification)`
  - 菜单"适合屏幕 ⌘0"调用 `fitToScreen()`；"放大 ⌘="、"缩小 ⌘-"以视口中心为锚点，分别调用 `zoomAt(factor: 1.25)` 和 `zoomAt(factor: 0.8)`
- `FrameStatsHUD`：Debug 默认显示，Release 通过 `--frame-stats` 开启。显示 fps、cpu_ms 与帧间隔 p50/p95；fps 取最近 1 秒实际渲染数，静止时 dirty 帧数低不是性能下降。记录器与渲染循环分离；CPU 提交耗时不等于 GPU 完成时间。
- 启动后如果没有打开任何文档，就打开应用包里的 `sample.jpg`；`documentOpened` 事件调用 `showDocumentInfo` 并 `markDirty()`。

- [ ] **Step 6: 构建并手动验证**

Run: `make run`
Expected（逐项确认）：
1. 窗口显示示例照片，四周是 `#141415` 背景，副标题为 `1024 × 1536 · sRGB (assumed) · 8 位`
2. 触控板双指拖动时，内容跟随手指移动；可以拖到图片完全离开视野（无限画布）
3. 双指捏合以光标为中心缩放；⌘0 回到适合屏幕
4. 把窗口拖到极小再拉大，不崩溃且画面恢复
5. 如果有外接非 Retina 屏，拖过去后图片视觉大小不变、不变糊
6. 打开一个改名为 `.jpg` 的文本文件，弹出"暂不支持这种图片格式"
7. 打开 A 后尝试超大/损坏 B：弹错但 A 的画面、信息、视图不变；关闭/重开窗口、最小化/恢复不访问已释放 layer
8. 打开 16 位样例，副标题/信息显示 16 位；源像素测试通过，而不是只凭肉眼判断精度

- [x] **Step 7: 提交**

```bash
git add crates/strata-render crates/strata-ffi apps/macos/Strata
git commit -m "feat(canvas): render to CAMetalLayer with trackpad pan/zoom and frame stats"
```

---

### Task 9: CI 与可移植性检查

**Files:**
- Create: `scripts/check-portability.sh`, `.github/workflows/ci.yml`
- Modify: `Makefile`（启用 `lint` 目标中的可移植性检查）

**Interfaces:**
- Consumes: 前面各任务的 crate 与 Makefile 目标

- [x] **Step 1: 写 `scripts/check-portability.sh`**

对 `crates/strata-pixels`、`crates/strata-core`、`crates/strata-color`、`crates/strata-io` 已存在的 `src/`、`tests/` 与 `Cargo.toml` 搜索以下模式：`target_os = "macos"`、`objc`、`metal`、`core-foundation`、`core_foundation`、`cocoa`、`windows-sys`、`winapi`。命中时打印文件与行号并以退出码 1 结束；未命中时打印 `portability check passed`。可通过环境变量 `STRATA_PORTABLE_CRATES` 覆盖检查目录（用于测试脚本本身）。该扫描只是静态防线，Linux 编译和依赖审计仍需执行。

- [x] **Step 2: 验证脚本能拦住违规**

Run:
```bash
scripts/check-portability.sh
T=$(mktemp -d) && mkdir -p $T/src && echo '#[cfg(target_os = "macos")] fn x(){}' > $T/src/lib.rs && STRATA_PORTABLE_CRATES=$T scripts/check-portability.sh; echo "exit=$?"
```
Expected: 第一条输出 `portability check passed`；第二条打印出违规行，并输出 `exit=1`

- [x] **Step 3: 写 `.github/workflows/ci.yml`**

触发条件：`push` 到 `main` 和所有 `pull_request`。两个 job：
- `linux`（开工验证后固定 runner）：checkout → 显式安装 rust-toolchain.toml 中锁定的 toolchain 与 components → `cargo fmt --all --check` → `cargo clippy --workspace --all-targets --locked -- -D warnings` → `cargo test --workspace --locked` → `scripts/check-portability.sh`。无 adapter 可跳过 GPU 测试，但日志明确区分通过/跳过。
- `macos`（开工验证后固定 Apple 芯片 runner 与 Xcode）：checkout → 安装同一 Rust toolchain → 安装锁定 XcodeGen → `make lint` → `make test`；该 job 不预先生成工程，用于持续验证 `test → project → rust` 的依赖完整性。至少首次与发布前执行不恢复构建缓存的冷构建。

两个 job 可缓存 registry 与 target，缓存键包含 OS、架构、Rust 版本、profile 和 Cargo.lock 哈希；不能靠缓存提供未入库的工程/绑定。GPU runner 不可用时保留独立本机 `STRATA_REQUIRE_GPU=1` 验收记录，不能把跳过记为渲染通过。

- [x] **Step 4: 本地跑一遍与 CI 相同的检查**

Run: `make lint && make test`
Expected: 全部通过（推送后的 CI 结果在 Task 11 确认）

- [x] **Step 5: 提交**

```bash
git add scripts/check-portability.sh .github/workflows/ci.yml Makefile
git commit -m "ci: add Linux core build and macOS app build with portability check"
```

---

### Task 10: 三项技术验证与结论

**Files:**
- Create: `scripts/make-perf-image.sh`, `scripts/make-p3-sample.sh`
- Create: `docs/research/2026-11-m0-bridge-uniffi.md`, `docs/research/2026-11-m0-rendering-wgpu-metal.md`, `docs/research/2026-11-m0-color-p3.md`
- Modify: `docs/plans/2026-11-02-strata-v1-roadmap.md`（只勾选有实测证据的 M0 项）、`docs/README.md`（加入三份真实结论的链接）

**Interfaces:**
- Consumes: Task 8 的 Release 应用（通过启动参数开启帧统计）

- [x] **Step 1: 写测试图生成脚本**

- `make-perf-image.sh`：`mkdir -p build && sips -z 6000 4000 docs/design/assets/sample-photo.jpg --out build/perf-24mp.jpg`
- `make-p3-sample.sh`：`sips --matchTo "/System/Library/ColorSync/Profiles/Display P3.icc" docs/design/assets/sample-photo.jpg --out build/sample-p3.jpg`
- 上述 sRGB 照片转 P3 只能验证配置文件路径，不能验证超 sRGB 色域。另在 color/io 测试夹具中生成带 P3 ICC 的原生纯红/绿/中性色块、带 alpha 的边缘、16 位完整灰阶与非端点样本，导出到 build 供人工比较；生成器与预期值纳入版本控制。

Run: `chmod +x scripts/make-*.sh && scripts/make-perf-image.sh && scripts/make-p3-sample.sh && sips -g pixelWidth -g pixelHeight build/perf-24mp.jpg`
Expected: `pixelWidth: 4000`、`pixelHeight: 6000`；`build/sample-p3.jpg` 已生成

- [x] **Step 2a: 本机程序驱动渲染性能验证**

已用 `make benchmark` 在 M4 主机上完成真实 Release 画布的固定轨迹 3×30 秒；三轮达到下面的 60Hz 提交间隔门槛，详见[报告与限制](../research/2026-11-m0-rendering-wgpu-metal.md)。脚本请求 60Hz，保留屏幕 nominal 刷新率，不用零散 HUD 帧估算连续性能。

- [ ] **Step 2b: 标准机与真实触控板复验**

用 `make run CONFIGURATION=Release FRAME_STATS=1`，打开 24MP 图，固定视口/屏幕刷新率，持续平移缩放 30 秒、重复 3 次；记录物理尺寸、冷/热状态、fps、帧间隔 p50/p95、cpu_ms 与峰值内存。60Hz 基准持续交互典型值 ≥ 58fps、p95 帧间隔 ≤ 20ms；不把静止时的 dirty 帧统计计入。未达标则保留 M0 渲染项未完成并分析，不能只写“留给 M1”。这不替代 M1 多层/超内存基准。

- [ ] **Step 3: 色彩验证**

Strata 与“预览”在同一 P3 屏幕、相同缩放显示照片及原生 P3 色块，关闭 True Tone/Night Shift 等自适应显示；用数码测色计相同模式测大色块内部，避免插值边缘，记录读数。每通道相差 ≤ 2（8 位读数）作为显示交叉验证；没有合适屏幕时明确“未验证”，不能凭截图勾选。另跑 Task 2/3/6 的 16 位样本与浮点参考测试；屏幕读数不能证明 16 位数据保真。

- [ ] **Step 4: 写三份结论**

每份一页，结构固定为：目标 / 方法 / 结果（附实测数据）/ 遇到的问题 / 对 M1 的结论。
- 桥接：UniFFI 0.32 的 foreign trait 回调与线程模型、Swift 6 并发模式是否需要降级、绑定生成和链接方式。
- 渲染：wgpu 30 在 `CAMetalLayer` 上的表面创建方式、24MP 下的帧率与 cpu_ms、没有 mipmap 时缩小显示的锯齿情况（M1 用分块缩略级别解决）。
- 色彩：源 8/16 位像素保留 → 同位深 ICC 显示转换 → 线性预乘浮点缓存 → sRGB 传输曲线输出与 P3 layer；记录原生广色域、alpha、16 位测试数据，以及 HEIC、灰度/CMYK ICC 留到 M1 的支持边界。

- [ ] **Step 5: 更新路线图与文档索引并提交**

```bash
git add scripts/make-perf-image.sh scripts/make-p3-sample.sh docs/research docs/plans/2026-11-02-strata-v1-roadmap.md docs/README.md
git commit -m "docs: record M0 technical validation results"
```

---

### Task 11: 接入 GitHub 私有仓库（已获发起人授权）

**Files:**
- Modify: `Cargo.toml`（`[workspace.package] repository`）、`README.md`（私有仓库与 CI 记录）、`apps/macos/project.yml`（如需调整 Bundle ID）

- [x] **Step 1: 确认目标与公开范围**：发起人指定已有仓库 `BugCooker-cooking/strata`，目前私有，完成成品后再开源；不自动更改可见性。行为准则私密邮箱在开放外部贡献前另行确定，不阻塞已授权的私有开发。

- [x] **Step 2: 填写仓库地址与开发状态并提交**（私有仓库以实际 CI 运行链接为准）

```bash
git add Cargo.toml README.md apps/macos/project.yml
git commit -m "chore: set repository metadata"
```

- [x] **Step 3: 关联已有远程仓库并推送**

已关联 `origin=https://github.com/BugCooker-cooking/strata.git`，推送 `main`。操作前核对目标与私有状态；后续增量推送沿用该授权，不再创建同名仓库。

- [x] **Step 4: 确认 CI 通过**

Run: `gh run watch --exit-status`
Expected: `linux` 与 `macos` 两个 job 都成功。失败时按日志修复，提交后重新推送，直到两个都通过。

已验证：提交 `0624958` 的[远程运行](https://github.com/BugCooker-cooking/strata/actions/runs/41410183235)两个 job 均成功。后续提交仍需检查自身结果；本机 GPU、屏幕和最低系统门槛独立保留。

---

## M0 完成标准对照（来自路线图 §3）

| 路线图要求 | 对应任务 |
|---|---|
| Git 仓库与 GitHub 项目 | Task 1、Task 11 |
| LICENSE（Apache-2.0）、README、CONTRIBUTING、行为准则、Issue / PR 模板 | Task 1 |
| `.gitignore`、rustfmt / clippy 配置 | Task 1、Task 9（clippy 在 `make lint` 中以 `-D warnings` 执行） |
| Rust workspace 与 crate | Task 1–6（按需创建，见 Global Constraints 最后一条） |
| Xcode 工程骨架（macOS 14） | Task 7 |
| Linux CI：核心编译、测试、格式与静态检查 | Task 9 |
| macOS CI：Rust 测试 + Xcode 构建 | Task 9 |
| 桥接验证：Swift 调用 Rust、Rust 回调 Swift | Task 6、Task 7 |
| 渲染验证：CAMetalLayer + wgpu、触控板平移缩放、60 帧 | Task 5、Task 8、Task 10 |
| 色彩验证：Display P3 图片与"预览"一致 | Task 2、Task 8、Task 10 |
| 位深：8/16 位源像素与 ICC 保留、显示副本不回写 | Task 1b、Task 2、Task 3、Task 6 |
| 打开失败原子性、回调锁外投递、画布释放 | Task 5、Task 6、Task 8 |
| 克隆后一条命令构建运行（`make run`） | Task 7、Task 8 |
| 干净检出直接 `make test`，缺少 GPU 不冒充通过 | Task 7、Task 9 |
| 三项技术验证各一页结论 | Task 10 |

> SwiftLint 配置在 M1 开始写面板时加入，路线图已同步。M0 没有实现命令/保存，因此“没有保存测试”不代表这些被推迟到 M8；M1a/M1b 必须按路线图补齐后才能进入公开预览。
