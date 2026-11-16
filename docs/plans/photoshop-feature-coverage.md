# Photoshop 功能覆盖跟踪

由 scripts/feature-coverage.py 从完整功能清单和独立证据状态生成。一个条目可能包含多个动作；全部动作达到标准才算验收。优先级决定顺序，不代表本任务可以省略后续功能。

旧清单的 ✗ 项保持“重新核对”，包含历史移除和外部服务依赖；没有自动当作已完成。实现记录与验收记录独立。

| ID | 类别 | 功能 | 顺序 | 实现 | 验收/证据 |
|---|---|---|---|---|---|
| PS-00-9768e5bc84 | Strata 自有基础 | 命令总线 + 功能注册表 | P0 | 已实现 | 当前设备已有证据；持续回归 · [当前实现与验证](2026-11-03-m1-closure.md) |
| PS-00-4282fbce73 | Strata 自有基础 | 每个命令自带说明书 | P0 | 已实现 | 当前设备已有证据；持续回归 · [当前实现与验证](2026-11-03-m1-closure.md) |
| PS-00-6a15d3790f | Strata 自有基础 | AI 接口层：浏览分类 / 读取说明 / 调用 / 查看画布与图层 | P0 | 已实现 | 当前设备已有证据；持续回归 · [当前实现与验证](../research/2026-11-m2b-local-editing-progress.md) |
| PS-00-a9af7235f5 | Strata 自有基础 | AI 聊天面板 + 模型接入 | P0 | 已实现 | 当前设备已有证据；持续回归 · [当前实现与验证](../research/2026-11-m2b-local-editing-progress.md) |
| PS-00-e5b005f04b | Strata 自有基础 | 操作权锁（AI 执行中 / 暂停 / 终止） | P0 | 已实现 | 当前设备已有证据；持续回归 · [当前实现与验证](../research/2026-11-m2b-local-editing-progress.md) |
| PS-00-aa907aaad8 | Strata 自有基础 | 统一历史记录（标注来源，AI 回合可整组撤销） | P0 | 已实现 | 当前设备已有证据；持续回归 · [当前实现与验证](../research/2026-11-m2b-local-editing-progress.md) |
| PS-00-7a6d3fc6af | Strata 自有基础 | 无限画布 | P0 | 已实现 | 当前设备已有证据；持续回归 · [当前实现与验证](2026-11-03-m1-closure.md) |
| PS-00-81f5847182 | Strata 自有基础 | GPU 渲染管线 + 分块存储 | P0 | 已实现 | 当前设备已有证据；持续回归 · [当前实现与验证](2026-11-03-m1-closure.md) |
| PS-00-a664122198 | Strata 自有基础 | Strata 工程文件格式 | P0 | 已实现 | 当前设备已有证据；持续回归 · [当前实现与验证](2026-11-03-m1-closure.md) |
| PS-00-6f14bdb57f | Strata 自有基础 | 自动保存与崩溃恢复 | P0 | 已实现 | 当前设备已有证据；持续回归 · [当前实现与验证](2026-11-03-m1-closure.md) |
| PS-00-7d7224e709 | Strata 自有基础 | 中英文界面 | P0 | 已实现 | 原生菜单选择简体中文或英语，下次启动生效。界面文本、文案参数和语言持久化见 P0 核对。 · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-00-960e444e99 | Strata 自有基础 | MCP Server | P1 | 待实现 | 待验收 |
| PS-00-4aa7af5b95 | Strata 自有基础 | 插件系统 | P3 | 待实现 | 待验收 |
| PS-01-51efedefef | 文件 | 新建文档（尺寸、分辨率、背景、预设） | P0 | 已实现 | 当前设备已有证据；持续回归 · [当前实现与验证](2026-11-03-m1-closure.md) |
| PS-01-9b14920078 | 文件 | 打开 PNG / JPEG / WebP / TIFF / HEIC / GIF / BMP | P0 | 已实现 | 当前设备已有证据；持续回归 · [当前实现与验证](2026-11-03-m1-closure.md) |
| PS-01-b6d7cbc94d | 文件 | 最近打开 | P0 | 已实现 | 当前设备已有证据；持续回归 · [当前实现与验证](2026-11-03-m1-closure.md) |
| PS-01-8f62b2552b | 文件 | 保存 / 另存为（Strata 格式） | P0 | 已实现 | 当前设备已有证据；持续回归 · [当前实现与验证](2026-11-03-m1-closure.md) |
| PS-01-894622b1ce | 文件 | 快速导出 / 导出为（格式、尺寸、质量） | P0 | 已实现 | Explicit export and real shared/versioned quick presets use the same PNG/TIFF/JPEG/WebP encoder with dimensions, ICC/DPI, precision/alpha and source protection. Full local core354+doc1/AI51, native116(115passed/1live-service opt-in skip), required Metal1, lint and140commands/209examples passed. Manual found size-field compression and sheet Cmd+A routing; both fixed, related11native tests and corrected arm64 Release passed. Sequential one-window manual saved/read JPEG83/sRGB/white256×384, wrote actualJPEG and original-sizePNG, verified metadata, defaults and clean source/history. Final process0/sourceSHA unchanged; only the test-created preference key removed to restore its original absence. Large-photo continuous performance/live-model quality and broader Photoshop remain separate. · [当前实现与验证](2026-11-03-m1-closure.md) · [Quick export preset implementation and local gates](../research/2026-11-m8-quick-export-progress.md) |
| PS-01-e7337088bf | 文件 | 置入图片为图层 | P0 | 已实现 | 当前设备已有证据；持续回归 · [当前实现与验证](2026-11-03-m1-closure.md) |
| PS-01-265b9a61ab | 文件 | 恢复到上次保存 | P1 | 已实现 | Source-bound file.revert/status implemented using real image/project reload, fresh matching read capability, explicit discard/history reset and optional confirmation expected_revision. Related22core,52AI,required Metal1,142commands/211examples, lint, final native15 and arm64 Release passed. Initial native failures were a canonical-path assertion and sheet-fixture-triggered XCTest host exit; identity assertion and XCTest-only termination guard fixed, same15 reran passed. One Release window manually verified cancel keeps2layers/dirty/Undo, apply restores original1layer/0,0/clean/Undo+Redo disabled; closing last window exited normally. SourceSHA and quick-preset absence unchanged, process0. No manual every-format/16-bit/corruption or sustained performance claim. · [Bound-source revert implementation and local progress](../research/2026-11-m8-revert-progress.md) |
| PS-01-40267007aa | 文件 | PSD 导入 | P1 | 待实现 | 待验收 |
| PS-01-261e177d1e | 文件 | PSD 导出 | P1 | 待实现 | 待验收 |
| PS-01-ab934d783e | 文件 | 打开 RAW | P2 | 待实现 | 待验收 |
| PS-01-b49b476915 | 文件 | 置入链接文件 | P2 | 待实现 | 待验收 |
| PS-01-a69a5778e5 | 文件 | 图层 / 画板导出为文件 | P2 | 待实现 | 待验收 |
| PS-01-a419a1eab8 | 文件 | 批处理、图像处理器 | P2 | 待实现 | 待验收 |
| PS-01-dd3fd3e0e8 | 文件 | 将文件载入堆栈 | P2 | 待实现 | 待验收 |
| PS-01-41c91ded13 | 文件 | 文件简介（EXIF / IPTC 元数据） | P2 | 待实现 | 待验收 |
| PS-01-d7bfe7b505 | 文件 | 打印 | P2 | 待实现 | 待验收 |
| PS-01-1b58fee46c | 文件 | PDF 导入 | P3 | 待实现 | 待验收 |
| PS-01-adb05bd33d | 文件 | 全景合成、合并到 HDR | P3 | 待实现 | 待验收 |
| PS-01-c35b70cb7c | 文件 | 打包、生成图像资源、变量与数据组 | P3 | 待实现 | 待验收 |
| PS-01-bdb7c564c5 | 文件 | 联系表、快捷批处理、视频帧到图层、旧版存储为 Web | ✗ | 重新核对 | 待验收 |
| PS-01-30d15e0488 | 文件 | 云文档、库、协作项目 | ✗ | 重新核对 | 待验收 |
| PS-02-47df18296a | 编辑 | 撤销 / 重做 / 历史记录面板 | P0 | 已实现 | 既有 M1/M3 真实历史/剪贴板实现；本机核心、原生与 AI 权限回归持续验证 · [M3 实现与权限](../research/2026-11-m3-layers-progress.md) · [当前入口核对](ai-editing-entrypoint-coverage.md) |
| PS-02-fc9f1fdaa0 | 编辑 | 历史快照 | P1 | 待实现 | 待验收 |
| PS-02-24d3726654 | 编辑 | 剪切 / 拷贝 / 合并拷贝 / 粘贴 / 原位粘贴 / 清除 | P0 | 已实现 | 既有 M1/M3 真实历史/剪贴板实现；本机核心、原生与 AI 权限回归持续验证 · [M3 实现与权限](../research/2026-11-m3-layers-progress.md) · [当前入口核对](ai-editing-entrypoint-coverage.md) |
| PS-02-44ec922e45 | 编辑 | 贴入 / 外部粘贴 | P1 | 待实现 | 待验收 |
| PS-02-03ea5fd9e6 | 编辑 | 填充：颜色 | P0 | 已实现 | 已复核现有 paint.fill 的选区/锁/8与16位/撤销/原生前景背景菜单及本轮颜色面板→填充→撤销证据；此前清单漏记已补齐。 · [填充实现与原生证据](../research/2026-11-m6-paint-retouch-progress.md) · [颜色面板与填充联动](../research/2026-11-m7-color-panel-progress.md) |
| PS-02-bfa246feb1 | 编辑 | 填充：内容识别 | P1 | 待实现 | 待验收 |
| PS-02-adcb808036 | 编辑 | 描边 | P1 | 待实现 | 待验收 |
| PS-02-f62f8ca281 | 编辑 | 渐隐 | P2 | 待实现 | 待验收 |
| PS-02-311ee1c4c5 | 编辑 | 定义画笔 / 图案 / 自定形状 | P2 | 待实现 | 待验收 |
| PS-02-57907a69fc | 编辑 | 偏好设置 | P0 | 已实现 | 当前 Mac 本机核心/内存/AI/原生/真实 Metal/Release 与单窗口保存、非法值拒绝、缓存释放、重启恢复通过。原生与 AI 共用设置命令；安全上下限、整组预算失败和跨标签版本保护生效。 · [M8 应用偏好与资源预算](../research/2026-11-m8-preferences-progress.md) |
| PS-02-91854349e1 | 编辑 | 自定义快捷键 / 菜单 / 工具栏 | P2 | 待实现 | 待验收 |
| PS-02-21ab73cf70 | 编辑 | 功能搜索 | P1 | 已实现 | Native Command-F searches the real catalog, displays the shared description and can insert a prompt into the AI draft. Bilingual lookup and unsupported-command checks passed in XCTest. One Release window confirmed Chinese lookup and insertion into the draft. · [Document duplication and feature search local checks](../research/2026-11-m8-duplicate-search-progress.md) |
| PS-02-b2fd2e2a88 | 编辑 | 清理缓存 | P2 | 部分实现 | 空闲缓存释放与源/历史块磁盘回收已接通；剪贴板与历史定向清除待实现。 · [M8 应用偏好与资源预算](../research/2026-11-m8-preferences-progress.md) |
| PS-02-bc773443d0 | 编辑 | 拼写检查、查找替换文本 | P3 | 待实现 | 待验收 |
| PS-03-ad24731ea8 | 图像 | 图像大小（多种重采样算法） | P0 | 已实现 | 最近邻、双线性和面积三种算法。现有层类型、蒙版和选区支持缩放。实际渐变样本的色码、重开、旧清单版本拒绝和一次撤销有本机证据。 · [尺寸重采样进展](../research/2026-11-m7-resize-progress.md) · [Retained text geometry / ordered crop](../research/2026-11-m7-text-geometry-progress.md) · [Subsequent native text/group manual scope](../research/data/2026-11-07-m7-native-text-guides-closure.json) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-03-45c2eea14c | 图像 | 画布大小 | P0 | 已实现 | 九锚点、原码/锁定结构/蒙版/选区、旧工程迁移与撤销通过；当前设备原生/Metal/Release 已有证据；持续回归 · [M7 画布大小](../research/2026-11-m7-canvas-progress.md) |
| PS-03-5f9716e5d8 | 图像 | 图像旋转 / 翻转 | P0 | 已实现 | 五种精确方向与任意角度覆盖现有图层类型，保留文本、填充、调整、样式及蒙版。裁剪后的填充支撑范围有回归。 · [M7 整图正交变换](../research/2026-11-m7-orientation-progress.md) · [General pixel-image angle](../research/2026-11-m7-rotate-progress.md) · [拉直与内切裁剪](../research/2026-11-m7-straighten-progress.md) · [画布参考线会话](../research/2026-11-m7-straighten-gesture-progress.md) · [Retained text geometry / ordered crop](../research/2026-11-m7-text-geometry-progress.md) · [Subsequent native text/group manual scope](../research/data/2026-11-07-m7-native-text-guides-closure.json) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-03-84941ac6e8 | 图像 | 显示全部 | P0 | 已实现 | 可见源内容/祖先样式边界、原码/蒙版/选区/撤销及当前设备原生/Metal/Release 已有证据；持续回归 · [M7 裁剪/显示全部](../research/2026-11-m7-crop-progress.md) |
| PS-03-51294954aa | 图像 | 裁切（去透明边） | P1 | 已实现 | Local core/AI/native subset/real Metal/Release and single-window four-side/no-op/undo evidence passed; ongoing regression. · [Transparent alpha trim](../research/2026-11-m7-trim-progress.md) |
| PS-03-0f4e217715 | 图像 | 复制文档 | P1 | 已实现 | Layered and visible-artboard copies use independent editors and have exact initial appearance, separate saved state and history, and bounded shared storage. Core/file/pixel/AI, real Metal, native and Release checks passed. One window verified both copy modes, X40 on the duplicate versus X0 on the source, and closing unsaved copies. · [Document duplication and feature search local checks](../research/2026-11-m8-duplicate-search-progress.md) |
| PS-03-691c261ce9 | 图像 | 自动色调 / 自动对比度 / 自动颜色 | P0 | 已实现 | 三种自动校正的核心/AI/原生/强制真实Metal/arm64 Release及单窗口菜单、70%中和预览取消/应用/撤销通过；当前设备已达增量验收检查点，持续大图性能、模型质量及完整PS门槛独立保留。 · [自动校正进展](../research/2026-11-m7-auto-corrections-progress.md) |
| PS-03-c10dd16a58 | 图像 | RGB 模式 | P0 | 已实现 | RGB 模式在检查器和统一文档查询中明确显示，位深与 ICC 独立报告。原生不支持的样本格式有转换指引。 · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-03-484489e467 | 图像 | 灰度模式 | P1 | 待实现 | 待验收 |
| PS-03-67a3559782 | 图像 | CMYK 模式 | P2 | 待实现 | 待验收 |
| PS-03-3e1da0c6ef | 图像 | Lab / 索引色 / 双色调 / 位图 | P3 | 待实现 | 待验收 |
| PS-03-e0d8c7ed11 | 图像 | 多通道 | ✗ | 重新核对 | 待验收 |
| PS-03-692d64115b | 图像 | 8 位 / 16 位 | P0 | 已实现 | 整文档双向转换、全部当前类型/样式颜色、蒙版/ICC/原码、锁/资源/取消/工程/撤销，以及 AI、原生、真实 Metal、Release 和单窗口菜单/面板验收通过；32位/HDR 为独立后续项。 · [文档位深转换](../research/2026-11-m7-bit-depth-progress.md) |
| PS-03-f31cf0825e | 图像 | 32 位（HDR） | P2 | 待实现 | 待验收 |
| PS-03-73a8b2cf0b | 图像 | 应用图像 | P2 | 待实现 | 待验收 |
| PS-03-e44976409a | 图像 | 计算 | P3 | 待实现 | 待验收 |
| PS-03-6e64fa5ccc | 图像 | 分析与测量 | P3 | 待实现 | 待验收 |
| PS-03-d47b3363b5 | 图像 | 陷印 | ✗ | 重新核对 | 待验收 |
| PS-04-430fc8e2db | 图层 | 新建图层 / 通过拷贝 / 通过剪切 / 背景转图层 | P0 | 已实现 | 本机图层、命令和合成测试覆盖该条目的各动作。历史与单窗口证据见 M3 及 P0 核对记录。 · [M3 计划](2026-11-05-m3-layers.md) · [M3 图层本机验收](../research/2026-11-m3-layers-progress.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-04-4e8bda1fd0 | 图层 | 复制 / 删除 / 重命名 / 颜色标签 | P0 | 已实现 | 本机图层、命令和合成测试覆盖该条目的各动作。历史与单窗口证据见 M3 及 P0 核对记录。 · [M3 计划](2026-11-05-m3-layers.md) · [M3 图层本机验收](../research/2026-11-m3-layers-progress.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-04-fe6945c814 | 图层 | 图层组（可嵌套）/ 取消编组 | P0 | 已实现 | 本机图层、命令和合成测试覆盖该条目的各动作。历史与单窗口证据见 M3 及 P0 核对记录。 · [M3 计划](2026-11-05-m3-layers.md) · [M3 图层本机验收](../research/2026-11-m3-layers-progress.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-04-54bcfbb2ac | 图层 | 显示隐藏 / 锁定（透明像素、像素、位置、全部） | P0 | 已实现 | 本机图层、命令和合成测试覆盖该条目的各动作。历史与单窗口证据见 M3 及 P0 核对记录。 · [M3 计划](2026-11-05-m3-layers.md) · [M3 图层本机验收](../research/2026-11-m3-layers-progress.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-04-a7fe5679e6 | 图层 | 不透明度 / 填充不透明度 | P0 | 已实现 | 本机图层、命令和合成测试覆盖该条目的各动作。历史与单窗口证据见 M3 及 P0 核对记录。 · [M3 计划](2026-11-05-m3-layers.md) · [M3 图层本机验收](../research/2026-11-m3-layers-progress.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-04-bd9b0e9d03 | 图层 | 混合模式（27 种） | P0 | 已实现 | 本机图层、命令和合成测试覆盖该条目的各动作。历史与单窗口证据见 M3 及 P0 核对记录。 · [M3 计划](2026-11-05-m3-layers.md) · [M3 图层本机验收](../research/2026-11-m3-layers-progress.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-04-e5e334c943 | 图层 | 排列顺序 / 对齐 / 分布 | P0 | 已实现 | 本机图层、命令和合成测试覆盖该条目的各动作。历史与单窗口证据见 M3 及 P0 核对记录。 · [M3 计划](2026-11-05-m3-layers.md) · [M3 图层本机验收](../research/2026-11-m3-layers-progress.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-04-e77c27785e | 图层 | 向下合并 / 合并可见 / 拼合图像 / 盖印 | P0 | 已实现 | 本机图层、命令和合成测试覆盖该条目的各动作。历史与单窗口证据见 M3 及 P0 核对记录。 · [M3 计划](2026-11-05-m3-layers.md) · [M3 图层本机验收](../research/2026-11-m3-layers-progress.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-04-d5e9c4039f | 图层 | 图层蒙版（添加、编辑、停用、应用、反相） | P0 | 已实现 | 添加/编辑/停用/应用/反相已有核心及原生证据；扩展几何继续补充 · [M3](../research/2026-11-m3-layers-progress.md) · [M6 蒙版绘制](../research/2026-11-m6-paint-retouch-progress.md) |
| PS-04-9101e755a1 | 图层 | 剪贴蒙版 | P0 | 已实现 | 本机图层、命令和合成测试覆盖该条目的各动作。历史与单窗口证据见 M3 及 P0 核对记录。 · [M3 计划](2026-11-05-m3-layers.md) · [M3 图层本机验收](../research/2026-11-m3-layers-progress.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-04-cb66199f15 | 图层 | 调整图层 | P0 | 已实现 | 曝光、色相／饱和度和 P0 全部调色类型已有本机、原生、AI 和 Metal 样例。空间调整和普通调色分别保留蒙版与参数。 · [M3 计划](2026-11-05-m3-layers.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) · [M5 调色与局部调整](../research/2026-11-m5-adjustments-progress.md) |
| PS-04-755f2b287b | 图层 | 填充图层：纯色 / 渐变 | P0 | 已实现 | 本机图层、命令和合成测试覆盖该条目的各动作。历史与单窗口证据见 M3 及 P0 核对记录。 · [M3 计划](2026-11-05-m3-layers.md) · [M3 图层本机验收](../research/2026-11-m3-layers-progress.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-04-af7f602330 | 图层 | 填充图层：图案 | P2 | 待实现 | 待验收 |
| PS-04-853e6d30f3 | 图层 | 栅格化 | P0 | 已实现 | P0 图层类型可显式栅格化。当前 Mac 文字单窗口验收覆盖栅格化、一次撤销与继续编辑。 · [M3 计划](2026-11-05-m3-layers.md) · [文字栅格化入口补验](../research/2026-11-m7-text-rasterization-entrypoints.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-04-d6c3930e2f | 图层 | 图层样式：投影、描边、外发光、颜色叠加、渐变叠加 | P0 | 已实现 | 本机图层、命令和合成测试覆盖该条目的各动作。历史与单窗口证据见 M3 及 P0 核对记录。 · [M3 计划](2026-11-05-m3-layers.md) · [M3 图层本机验收](../research/2026-11-m3-layers-progress.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-04-6084cae48d | 图层 | 图层样式：内阴影、内发光、斜面浮雕、光泽、图案叠加；样式预设 | P1 | 待实现 | 待验收 |
| PS-04-e65a84f08f | 图层 | 链接图层 | P1 | 待实现 | 待验收 |
| PS-04-288c65e42f | 图层 | 混合颜色带（Blend If） | P1 | 待实现 | 待验收 |
| PS-04-ef23225e49 | 图层 | 挖空 | P3 | 待实现 | 待验收 |
| PS-04-c9a10eae37 | 图层 | 智能对象：转换、编辑内容、替换内容、栅格化 | P1 | 待实现 | 待验收 |
| PS-04-9b3a04c666 | 图层 | 智能滤镜 | P1 | 待实现 | 待验收 |
| PS-04-9383ee3abd | 图层 | 矢量蒙版 | P1 | 待实现 | 待验收 |
| PS-04-dd95b3b697 | 图层 | 图层筛选与查找 | P1 | 待实现 | 待验收 |
| PS-04-53f23afed6 | 图层 | 画板 | P1 | 待实现 | 待验收 |
| PS-04-f22dbf0e07 | 图层 | 图层复合 | P2 | 待实现 | 待验收 |
| PS-04-fc1ecbb499 | 图层 | 修边（去边、移除黑 / 白杂边） | P2 | 待实现 | 待验收 |
| PS-04-d31acdcaa9 | 图层 | 视频图层 | ✗ | 重新核对 | 待验收 |
| PS-05-24f905f241 | 选择 | 矩形 / 椭圆选框 | P0 | 已实现 | 本机核心/原生证据；真实模型与固定性能门槛另记 · [M4 实现与验证](../research/2026-11-m4-selection-progress.md) |
| PS-05-8885dd3689 | 选择 | 套索 / 多边形套索 | P0 | 已实现 | 本机核心/原生证据；真实模型与固定性能门槛另记 · [M4 实现与验证](../research/2026-11-m4-selection-progress.md) |
| PS-05-d2a511da9e | 选择 | 魔棒 | P0 | 已实现 | 本机核心/原生证据；真实模型与固定性能门槛另记 · [M4 实现与验证](../research/2026-11-m4-selection-progress.md) |
| PS-05-825f270266 | 选择 | 快速选择 | P0 | 已实现 | 本机核心/原生证据；真实模型与固定性能门槛另记 · [M4 实现与验证](../research/2026-11-m4-selection-progress.md) |
| PS-05-f88b2ecb2f | 选择 | 选择主体 🤖 | P0 | 已实现 | 本机推理/模型管理；困难质量集待验收 · [M2b](../research/2026-11-m2b-local-editing-progress.md) · [M4 模型管理](../research/2026-11-m4-selection-progress.md) |
| PS-05-c3053c1de1 | 选择 | 全选 / 取消选择 / 重新选择 / 反选 | P0 | 已实现 | 本机核心/原生证据；真实模型与固定性能门槛另记 · [M4 实现与验证](../research/2026-11-m4-selection-progress.md) |
| PS-05-a8345e1798 | 选择 | 选区相加 / 相减 / 交叉 | P0 | 已实现 | 本机核心/原生证据；真实模型与固定性能门槛另记 · [M4 实现与验证](../research/2026-11-m4-selection-progress.md) |
| PS-05-2cd4b1dc43 | 选择 | 羽化 / 扩展 / 收缩 / 平滑 / 边界 | P0 | 已实现 | 本机核心/原生证据；真实模型与固定性能门槛另记 · [M4 实现与验证](../research/2026-11-m4-selection-progress.md) |
| PS-05-1f294f54d0 | 选择 | 变换选区 | P0 | 已实现 | 本机核心/原生证据；真实模型与固定性能门槛另记 · [M4 实现与验证](../research/2026-11-m4-selection-progress.md) |
| PS-05-9bfbc83e75 | 选择 | 磁性套索 | P1 | 待实现 | 待验收 |
| PS-05-aa44dc16d9 | 选择 | 对象选择 🤖 | P1 | 待实现 | 待验收 |
| PS-05-9ac4e17211 | 选择 | 选择天空 🤖 | P1 | 待实现 | 待验收 |
| PS-05-38d4ee4918 | 选择 | 按文字描述选择（如"选中左边的人"）🤖 | P1 | 待实现 | 待验收 |
| PS-05-8d060c56cd | 选择 | 扩大选取 / 选取相似 | P1 | 待实现 | 待验收 |
| PS-05-2998b53723 | 选择 | 色彩范围 | P1 | 待实现 | 待验收 |
| PS-05-4ed8bb8e25 | 选择 | 快速蒙版 | P1 | 待实现 | 待验收 |
| PS-05-60905d6faf | 选择 | 存储 / 载入选区（Alpha 通道） | P1 | 待实现 | 待验收 |
| PS-05-f7f00647a1 | 选择 | 选择并遮住（细化边缘，如头发） | P1 | 待实现 | 待验收 |
| PS-05-6a8c7914d1 | 选择 | 单行 / 单列选框 | P3 | 待实现 | 待验收 |
| PS-05-96d3b2c5d9 | 选择 | 焦点区域 | P3 | 待实现 | 待验收 |
| PS-06-d51749f4f4 | 调整 | 亮度 / 对比度 | P0 | 已实现 | 本机核心/原生/Metal 证据；实时预览、性能与模型任务另记 · [M5 进展](../research/2026-11-m5-adjustments-progress.md) |
| PS-06-4341979e35 | 调整 | 色阶 | P0 | 已实现 | 本机核心/原生/Metal 证据；实时预览、性能与模型任务另记 · [M5 进展](../research/2026-11-m5-adjustments-progress.md) |
| PS-06-937dc7bf30 | 调整 | 曲线 | P0 | 已实现 | 本机核心/原生/Metal 证据；实时预览、性能与模型任务另记 · [M5 进展](../research/2026-11-m5-adjustments-progress.md) |
| PS-06-b941862025 | 调整 | 曝光度 | P0 | 已实现 | 本机数值/原生专项已有证据；完整回归、手工与连续性能按对应进展记录 · [本机实现/验收记录](../research/2026-11-m2b-local-editing-progress.md) |
| PS-06-e7e1ea6652 | 调整 | 自然饱和度 + 色温 / 色调 | P0 | 已实现 | 核心/原生/Metal 回归；实时预览、固定性能和真实模型另记 · [M5 第二批](../research/2026-11-m5-adjustments-progress.md) |
| PS-06-65bc8e9d6e | 调整 | 色相 / 饱和度 | P0 | 已实现 | 本机数值/原生专项已有证据；完整回归、手工与连续性能按对应进展记录 · [本机实现/验收记录](2026-11-05-m3-layers.md) |
| PS-06-cdc392db20 | 调整 | 色彩平衡 | P0 | 已实现 | 核心/原生/Metal 回归；实时预览、固定性能和真实模型另记 · [M5 第二批](../research/2026-11-m5-adjustments-progress.md) |
| PS-06-47d88357c5 | 调整 | 黑白 | P0 | 已实现 | 核心/原生/Metal 回归；实时预览、固定性能和真实模型另记 · [M5 第二批](../research/2026-11-m5-adjustments-progress.md) |
| PS-06-2263716466 | 调整 | 照片滤镜 | P0 | 已实现 | 核心/原生/Metal 回归；实时预览、固定性能和真实模型另记 · [M5 第二批](../research/2026-11-m5-adjustments-progress.md) |
| PS-06-7fec4dc313 | 调整 | 通道混合器 | P0 | 已实现 | 核心/原生/Metal 回归；实时预览、固定性能和真实模型另记 · [M5 第二批](../research/2026-11-m5-adjustments-progress.md) |
| PS-06-39bbb078d5 | 调整 | 反相 / 色调分离 / 阈值 | P0 | 已实现 | 核心/原生/Metal 回归；实时预览、固定性能和真实模型另记 · [M5 第二批](../research/2026-11-m5-adjustments-progress.md) |
| PS-06-1769d6d1d6 | 调整 | 渐变映射 | P0 | 已实现 | 核心/原生/Metal 回归；实时预览、固定性能和真实模型另记 · [M5 第二批](../research/2026-11-m5-adjustments-progress.md) |
| PS-06-63924bf039 | 调整 | 可选颜色 | P0 | 已实现 | 核心/原生/Metal 回归；实时预览、固定性能和真实模型另记 · [M5 第二批](../research/2026-11-m5-adjustments-progress.md) |
| PS-06-ef2316cc3c | 调整 | 阴影 / 高光 | P0 | 已实现 | 本机核心跨块/组/剪贴/Metal 回归；质量集与性能门槛另记 · [M5 局部阴影高光](../plans/2026-11-05-m5-local-tone.md) · [M5 调色进展](../research/2026-11-m5-adjustments-progress.md) |
| PS-06-be1d0be625 | 调整 | 去色 | P0 | 已实现 | 核心/原生/Metal 回归；实时预览、固定性能和真实模型另记 · [M5 第二批](../research/2026-11-m5-adjustments-progress.md) |
| PS-06-3032971663 | 调整 | 清晰度 / 去朦胧 / 纹理 | P1 | 待实现 | 待验收 |
| PS-06-2888ee744e | 调整 | 颜色查找（LUT） | P1 | 待实现 | 待验收 |
| PS-06-7b4433741f | 调整 | 匹配颜色 / 替换颜色 / 色调均化 | P1 | 待实现 | 待验收 |
| PS-06-ff679aec47 | 调整 | 调整预设 | P1 | 待实现 | 待验收 |
| PS-06-77f246179b | 调整 | HDR 色调 | P3 | 待实现 | 待验收 |
| PS-07-7fd32754e9 | 修复与修饰 | 污点修复画笔 | P0 | 已实现 | 近邻匹配、创建纹理及内容识别覆盖双位深、保护区、确定重放、撤销和结构色带。逐项证据见 P0 核对记录。 · [M6 自动近邻污点修复](../research/2026-11-m6-paint-retouch-progress.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-07-2eddb4a1cf | 修复与修饰 | 修复画笔 | P0 | 已实现 | 本机核心/原生输入/Metal/Release 通过；手工笔触、连续性能与照片质量仍待验收 · [M6 采样修复](../research/2026-11-m6-paint-retouch-progress.md) |
| PS-07-ec6361e1f3 | 修复与修饰 | 仿制图章 | P0 | 已实现 | 本机原始/合成取样、对齐、像素/原生/Metal/单窗口证据；连续性能另记 · [M6 仿制图章](../research/2026-11-m6-paint-retouch-progress.md) |
| PS-07-74865c42fd | 修复与修饰 | 减淡 / 加深 / 海绵 | P0 | 已实现 | 本机核心/原生输入/Metal/Release 通过；手工、照片质量与连续性能仍待验收 · [M6 局部色调笔触](../research/2026-11-m6-paint-retouch-progress.md) |
| PS-07-99d39b0e3f | 修复与修饰 | 修补工具 | P1 | 待实现 | 待验收 |
| PS-07-567e47777f | 修复与修饰 | 内容识别填充 | P1 | 待实现 | 待验收 |
| PS-07-9411b77a01 | 修复与修饰 | 移除工具 🤖 | P1 | 待实现 | 待验收 |
| PS-07-a39f54a54c | 修复与修饰 | 红眼工具 | P1 | 待实现 | 待验收 |
| PS-07-2ad3989bf6 | 修复与修饰 | 模糊 / 锐化 / 涂抹工具 | P1 | 待实现 | 待验收 |
| PS-07-a2d1d27aed | 修复与修饰 | 液化 | P1 | 待实现 | 待验收 |
| PS-07-dfaf73f366 | 修复与修饰 | 内容识别移动 | P2 | 待实现 | 待验收 |
| PS-07-a63d0d97e2 | 修复与修饰 | 仿制源面板 | P2 | 待实现 | 待验收 |
| PS-07-ba6e326669 | 修复与修饰 | 图案图章 | P3 | 待实现 | 待验收 |
| PS-08-13d3081dfa | 绘画 | 画笔（大小、硬度、不透明度、流量、间距、压感） | P0 | 已实现 | 本机数值/原生专项已有证据；完整回归、手工与连续性能按对应进展记录 · [本机实现/验收记录](../research/2026-11-m6-paint-retouch-progress.md) |
| PS-08-05cf56410b | 绘画 | 橡皮擦 | P0 | 已实现 | 本机像素/蒙版/锁/历史/Metal 证据；连续性能另记 · [M6 绘画证据](../research/2026-11-m6-paint-retouch-progress.md) |
| PS-08-af9951e405 | 绘画 | 油漆桶 | P0 | 已实现 | 本机核心/原生/Metal/单窗口证据；大图连续性能另记 · [M6 第二批](../research/2026-11-m6-paint-retouch-progress.md) |
| PS-08-739094bd86 | 绘画 | 渐变（线性 / 径向 / 角度 / 对称 / 菱形） | P0 | 已实现 | 本机核心/原生/Metal/单窗口证据；大图连续性能另记 · [M6 第二批](../research/2026-11-m6-paint-retouch-progress.md) |
| PS-08-6f11ed20eb | 绘画 | 吸管 | P0 | 已实现 | 本机核心/原生/Metal/单窗口证据；大图连续性能另记 · [M6 第二批](../research/2026-11-m6-paint-retouch-progress.md) |
| PS-08-3ce0368d80 | 绘画 | 拾色器、前景色 / 背景色 | P0 | 已实现 | 前景/背景 get/set/swap/reset 与原生拾色器/吸管/X/D 统一；当前设备核心、AI/FFI/完整原生/Metal/Release 与单窗口已有证据；持续回归 · [M6 绘画证据](../research/2026-11-m6-paint-retouch-progress.md) · [工具调色板与 AI](../research/2026-11-m7-paint-colors-progress.md) |
| PS-08-86b51af036 | 绘画 | 颜色面板 | P0 | 已实现 | 常驻RGB/HSB/HEX前景背景面板、共享格式读写命令、核心/AI/原生/真实Metal/Release与单窗口改色/填充/撤销通过；窄面板标签和控件遮挡已修正并重验。 · [常驻颜色面板](../research/2026-11-m7-color-panel-progress.md) |
| PS-08-92b84304ee | 绘画 | 颜色取样器 | P1 | 待实现 | 待验收 |
| PS-08-8aec3995bf | 绘画 | 色板 | P1 | 待实现 | 待验收 |
| PS-08-f77e30ee1a | 绘画 | 铅笔 | P1 | 待实现 | 待验收 |
| PS-08-94f49a1012 | 绘画 | 画笔设置（形状动态、散布、纹理、双重画笔……） | P1 | 待实现 | 待验收 |
| PS-08-1c4da8d02e | 绘画 | 画笔 / 渐变预设管理 | P1 | 待实现 | 待验收 |
| PS-08-c236d7a8cc | 绘画 | 图案预设管理 | P2 | 待实现 | 待验收 |
| PS-08-d00f4ae949 | 绘画 | ABR 画笔导入 | P2 | 待实现 | 待验收 |
| PS-08-d5cde59fb9 | 绘画 | 背景橡皮擦 / 魔术橡皮擦 | P2 | 待实现 | 待验收 |
| PS-08-5bc77239a4 | 绘画 | 颜色替换 | P2 | 待实现 | 待验收 |
| PS-08-2b199ba19e | 绘画 | 历史记录画笔 | P2 | 待实现 | 待验收 |
| PS-08-20a38a31c0 | 绘画 | 混合器画笔 | P3 | 待实现 | 待验收 |
| PS-08-8917363943 | 绘画 | 历史记录艺术画笔 | ✗ | 重新核对 | 待验收 |
| PS-09-7cc6b0bb65 | 变换与裁剪 | 移动工具（含吸附） | P0 | 已实现 | V 拖动、数值与 AI 共用可见层、画板和参考线吸附，本机测试及单窗口检查通过。 · [M7 移动与几何边界](../research/2026-11-m7-transform-progress.md) · [Guides and snap local progress](../research/2026-11-m7-guides-snap-progress.md) · [Corrected native text / guide closure](../research/2026-11-m7-group-transform-progress.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) · [M7 可见图层间吸附](../research/2026-11-m7-smart-guides-progress.md) |
| PS-09-4f3d9835a7 | 变换与裁剪 | 自由变换：缩放 / 旋转 / 斜切 / 扭曲 / 透视 / 翻转 | P0 | 已实现 | 缩放、旋转、斜切、扭曲、透视、翻转及多选和蒙版进入统一核心。可编辑图层保留原定义，嵌套同选只变换一次，锁或无效几何原子拒绝。 · [M7 整层变换/视图检查点](../research/2026-11-m7-transform-progress.md) · [M7 选区内像素](../research/2026-11-m7-selected-transform-progress.md) · [画布手柄实施与验收](../research/2026-11-m7-transform-handles-progress.md) · [Retained text geometry / ordered crop](../research/2026-11-m7-text-geometry-progress.md) · [Editable nested group geometry](../research/2026-11-m7-group-transform-progress.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-09-7d349372b5 | 变换与裁剪 | 裁剪工具（拉直、比例、是否删除裁剪像素） | P0 | 已实现 | 覆盖拖框、比例和八手柄，保留／删除均有撤销证据。角度、两点和画布拉直支持所有现有图层类型。 · [M7 裁剪/显示全部](../research/2026-11-m7-crop-progress.md) · [M7 裁剪拖动会话](../research/2026-11-m7-crop-gesture-progress.md) · [M7 裁剪手柄](../research/2026-11-m7-crop-handles-progress.md) · [Destructive crop support](../research/2026-11-m7-crop-delete-progress.md) · [拉直与内切裁剪](../research/2026-11-m7-straighten-progress.md) · [画布参考线会话](../research/2026-11-m7-straighten-gesture-progress.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-09-64f3d2a3eb | 变换与裁剪 | 变形 | P1 | 待实现 | 待验收 |
| PS-09-073c032f33 | 变换与裁剪 | 透视裁剪 | P1 | 待实现 | 待验收 |
| PS-09-6891bcf56f | 变换与裁剪 | 操控变形 | P2 | 待实现 | 待验收 |
| PS-09-cfef658a75 | 变换与裁剪 | 透视变形 | P2 | 待实现 | 待验收 |
| PS-09-bf3489a74b | 变换与裁剪 | 内容识别缩放 | P2 | 待实现 | 待验收 |
| PS-09-b8404e61aa | 变换与裁剪 | 自动对齐图层 | P2 | 待实现 | 待验收 |
| PS-09-d278290204 | 变换与裁剪 | 自动混合图层 | P3 | 待实现 | 待验收 |
| PS-09-71958254c4 | 变换与裁剪 | 旋转对象 🤖（2026 新增） | P3 | 待实现 | 待验收 |
| PS-10-0e134f5a85 | 滤镜 | 高斯模糊 | P0 | 已实现 | 本机数值/原生专项已有证据；完整回归、手工与连续性能按对应进展记录 · [M6 基础滤镜/预览](../research/2026-11-m6-filters-progress.md) |
| PS-10-5785be70cb | 滤镜 | USM 锐化 | P0 | 已实现 | 本机数值/原生专项已有证据；完整回归、手工与连续性能按对应进展记录 · [M6 基础滤镜/预览](../research/2026-11-m6-filters-progress.md) |
| PS-10-395d09a3e1 | 滤镜 | 添加杂色 | P0 | 已实现 | 本机数值/原生专项已有证据；完整回归、手工与连续性能按对应进展记录 · [M6 基础滤镜/预览](../research/2026-11-m6-filters-progress.md) |
| PS-10-03bd81f36a | 滤镜 | 智能锐化 | P1 | 待实现 | 待验收 |
| PS-10-10e03c578e | 滤镜 | 高反差保留 | P1 | 已实现 | 工作RGB/线性P3双空间、独立数值/双位深/ICC/选区/资源/撤销和AI调用通过；原生预览、强制真实Metal、arm64 Release及单窗口两种模式应用/撤销通过，完整PS与持续性能门槛独立保留。 · [高反差保留进展](../research/2026-11-m7-high-pass-progress.md) |
| PS-10-c323ce2dc1 | 滤镜 | 减少杂色 / 中间值 / 蒙尘与划痕 | P1 | 待实现 | 待验收 |
| PS-10-4cd844c0cb | 滤镜 | 动感模糊 / 表面模糊 | P1 | 待实现 | 待验收 |
| PS-10-a5da48d57a | 滤镜 | 最大值 / 最小值 | P1 | 待实现 | 待验收 |
| PS-10-388913a7bc | 滤镜 | 径向模糊 / 镜头模糊 / 方框模糊 / 平均 | P2 | 待实现 | 待验收 |
| PS-10-7b85bb5f1a | 滤镜 | 模糊画廊：场景 / 光圈 / 移轴 / 路径 / 旋转 | P2 | 待实现 | 待验收 |
| PS-10-96038cde01 | 滤镜 | 镜头校正 | P2 | 待实现 | 待验收 |
| PS-10-9ce22316bf | 滤镜 | 位移 | P2 | 待实现 | 待验收 |
| PS-10-14619cadb7 | 滤镜 | 马赛克 | P2 | 待实现 | 待验收 |
| PS-10-4910186cb0 | 滤镜 | 云彩 | P2 | 待实现 | 待验收 |
| PS-10-9be49e6137 | 滤镜 | 扭曲组：置换 / 挤压 / 极坐标 / 波纹 / 切变 / 球面化 / 旋转扭曲 / 波浪 / 水波 | P3 | 待实现 | 待验收 |
| PS-10-344b92cabe | 滤镜 | 像素化组其余：彩色半调 / 晶格化 / 点状化 / 碎片 / 铜版雕刻 | P3 | 待实现 | 待验收 |
| PS-10-75ea494817 | 滤镜 | 渲染组：镜头光晕 / 光照效果 / 纤维 / 分层云彩 | P3 | 待实现 | 待验收 |
| PS-10-0672d98893 | 滤镜 | 风格化组：浮雕 / 查找边缘 / 油画 / 风 / 扩散 / 曝光过度 / 拼贴 / 凸出 / 等高线 | P3 | 待实现 | 待验收 |
| PS-10-d4877d09e6 | 滤镜 | 自定 / HSB/HSL | P3 | 待实现 | 待验收 |
| PS-10-7c8555eca5 | 滤镜 | 自适应广角 / 消失点 | P3 | 待实现 | 待验收 |
| PS-10-c5d0281f10 | 滤镜 | 滤镜库（47 个旧式艺术滤镜） | P3 | 待实现 | 待验收 |
| PS-10-dca9e3ce4b | 滤镜 | 火焰 / 树 / 图片框 | ✗ | 重新核对 | 待验收 |
| PS-10-1e9f8da0f7 | 滤镜 | 防抖 | ✗ | 重新核对 | 待验收 |
| PS-11-4490121dad | 文字 | 横排文字（点文字 / 段落文字） | P0 | 已实现 | 可编辑横排点文字及段落文字具有 CJK 排版、缺字提示、工程往返与修改后保留几何的证据。 · [可编辑文字进展](../research/2026-11-m7-editable-text-progress.md) · [Retained text geometry / ordered crop](../research/2026-11-m7-text-geometry-progress.md) · [Text / group native acceptance](../research/2026-11-m7-group-transform-progress.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-11-a524877cb6 | 文字 | 字体、字号、颜色、行距、字距、对齐 | P0 | 已实现 | 内置字体、字号、颜色、行距、字距、对齐均有实际属性、排版和原生测试。可用实际参数修改并撤销。 · [可编辑文字进展](../research/2026-11-m7-editable-text-progress.md) · [Retained text geometry / ordered crop](../research/2026-11-m7-text-geometry-progress.md) · [Text / group native acceptance](../research/2026-11-m7-group-transform-progress.md) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-11-cf9b897491 | 文字 | 栅格化文字 | P0 | 已实现 | 可编辑文字共用 layer.rasterize；AI 发现/精确批准、旋转与删除裁剪后 16 位 P3 显示码值、整轮撤销及原生撤销后继续编辑均通过本机自动化。当前 Mac 单窗口点文字栅格化、一次撤销恢复中文原文并继续编辑通过，样例恢复干净单层后退出；几何/裁剪的组合来自自动化证据。 · [可编辑文字进展](../research/2026-11-m7-editable-text-progress.md) · [文字栅格化入口补验](../research/2026-11-m7-text-rasterization-entrypoints.md) |
| PS-11-c6c65f4de6 | 文字 | 直排文字 | P1 | 待实现 | 待验收 |
| PS-11-0a97b13dc8 | 文字 | 文字变形 | P1 | 待实现 | 待验收 |
| PS-11-4bfa8a3abe | 文字 | 路径文字 | P2 | 待实现 | 待验收 |
| PS-11-f3c09ca56f | 文字 | 字符样式 / 段落样式 | P2 | 待实现 | 待验收 |
| PS-11-346810064b | 文字 | OpenType 特性 | P2 | 待实现 | 待验收 |
| PS-11-07a96c386e | 文字 | 文字转形状 / 创建工作路径 | P2 | 待实现 | 待验收 |
| PS-11-d0e9236cfa | 文字 | 文字蒙版工具 | P2 | 待实现 | 待验收 |
| PS-11-b2feb2a72d | 文字 | 字形面板 | P3 | 待实现 | 待验收 |
| PS-11-0907b4762e | 文字 | 匹配字体 🤖 | P3 | 待实现 | 待验收 |
| PS-12-417500cf30 | 矢量与路径 | 形状：矩形（含圆角）/ 椭圆 / 直线 / 多边形 / 三角形 | P1 | 待实现 | 待验收 |
| PS-12-a132c853ab | 矢量与路径 | 钢笔 / 弯度钢笔 / 自由钢笔 | P1 | 待实现 | 待验收 |
| PS-12-595e7321ce | 矢量与路径 | 添加 / 删除 / 转换锚点；路径选择 / 直接选择 | P1 | 待实现 | 待验收 |
| PS-12-cfd8ae8285 | 矢量与路径 | 路径面板、路径与选区互转 | P1 | 待实现 | 待验收 |
| PS-12-2e23400f5e | 矢量与路径 | 形状组合（合并 / 减去 / 相交） | P2 | 待实现 | 待验收 |
| PS-12-9bae2bd626 | 矢量与路径 | 自定形状 | P2 | 待实现 | 待验收 |
| PS-12-afd1a689fb | 矢量与路径 | SVG 导入导出 | P2 | 待实现 | 待验收 |
| PS-13-bbaac8cf76 | RAW 照片显影 | RAW 解码（主流相机） | P2 | 待实现 | 待验收 |
| PS-13-f7bdbf0b8e | RAW 照片显影 | 基本面板：白平衡、曝光、对比、高光、阴影、白色、黑色、纹理、清晰度、去朦胧 | P2 | 待实现 | 待验收 |
| PS-13-fc3a065bd4 | RAW 照片显影 | 曲线 / 混色器（HSL）/ 颜色分级 | P2 | 待实现 | 待验收 |
| PS-13-31102e180c | RAW 照片显影 | 细节：锐化、降噪 | P2 | 待实现 | 待验收 |
| PS-13-41b283aec3 | RAW 照片显影 | 效果：颗粒、晕影 | P2 | 待实现 | 待验收 |
| PS-13-f1de1e2621 | RAW 照片显影 | 预设 | P2 | 待实现 | 待验收 |
| PS-13-0529cd2d51 | RAW 照片显影 | 光学：镜头配置文件、色差、暗角 | P3 | 待实现 | 待验收 |
| PS-13-4592c9e11f | RAW 照片显影 | 几何：Upright 透视校正 | P3 | 待实现 | 待验收 |
| PS-13-1b8faab964 | RAW 照片显影 | AI 蒙版（主体 / 天空 / 人物）🤖、AI 降噪 🤖、镜头模糊 🤖 | P3 | 待实现 | 待验收 |
| PS-14-cf2715aeb6 | 视图与导航 | 缩放 / 抓手 / 适合屏幕 / 100% | P0 | 已实现 | 既有 M1 导航；绝对 100% 命令的 Retina 数值与单窗口实看通过，保持文档干净 · [M7 整层变换/视图检查点](../research/2026-11-m7-transform-progress.md) |
| PS-14-c9a6761937 | 视图与导航 | 多文档标签页 | P0 | 已实现 | M1b delivered independent native document tabs. Current local tests and one-window acceptance reconfirmed tab switching, independent edits/history and unsaved close prompts. AI context is bound to the owning document, and a duplicate starts a separate session. · [M1b workspace closure](2026-11-03-workspace-and-m1b.md) · [Document duplication and feature search local checks](../research/2026-11-m8-duplicate-search-progress.md) |
| PS-14-de2fd3ca59 | 视图与导航 | 标尺 / 参考线 / 对齐吸附 | P0 | 已实现 | 六种标尺单位、自定义原点、参考线增删改和锁定、画板／图层边缘与中心吸附已有本机和单窗口证据。 · [Guides and snap local progress](../research/2026-11-m7-guides-snap-progress.md) · [Corrected native text / guide closure](../research/2026-11-m7-group-transform-progress.md) · [Ruler units and custom origin](../research/2026-11-m7-ruler-units-progress.md) · [Visible layer reference increment](../research/2026-11-m7-smart-guides-progress.md) · [Owned ruler-corner origin drag](../research/2026-11-m7-ruler-origin-drag-progress.md) · [Single-window smart snap and origin acceptance](../research/data/2026-11-09-m7-smart-origin-native-closure.json) · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-14-810f37be85 | 视图与导航 | 旋转视图 | P1 | 待实现 | 待验收 |
| PS-14-15d282cc9a | 视图与导航 | 智能参考线 | P1 | 部分实现 | Visible raw-content layer/group edge-center snapping and exact-candidate overlays retain local core/AI/FFI/Metal/native/lint/examples/Release evidence. Current27e0c8c single-window manual V drag aligned duplicate left edge to visible original center atX512/Y0 with artboard snap off and zero guides; one Undo restored0/0. Numeric proposalX509 also resolved512. Undo duplicate/default restoration/clean document/normal quit/process0/sourceSHA passed. CUA observes after mouse release, so held smart-line appearance/latency is not claimed. Equal spacing, angle/size suggestions and sustained large-photo feedback remain open. · [Smart layer references progress](../research/2026-11-m7-smart-guides-progress.md) · [Single-window smart snap and origin acceptance](../research/data/2026-11-09-m7-smart-origin-native-closure.json) |
| PS-14-3e68cd79ce | 视图与导航 | 网格 / 像素网格 | P1 | 待实现 | 待验收 |
| PS-14-8ae0da2a42 | 视图与导航 | 导航器 | P1 | 待实现 | 待验收 |
| PS-14-09e4a9ec7d | 视图与导航 | 多窗口排列 / 屏幕模式 | P2 | 待实现 | 待验收 |
| PS-14-918e81018b | 视图与导航 | 校样颜色 / 色域警告 | P2 | 待实现 | 待验收 |
| PS-14-8430a881e8 | 视图与导航 | 新建参考线版面 | P3 | 待实现 | 待验收 |
| PS-15-d3094c6954 | 通道 | 查看单个颜色通道 | P1 | 待实现 | 待验收 |
| PS-15-b99621b074 | 通道 | Alpha 通道（存储选区） | P1 | 待实现 | 待验收 |
| PS-15-c4509c8344 | 通道 | 复制通道、通道转选区 | P1 | 待实现 | 待验收 |
| PS-15-117606acae | 通道 | 专色通道 | P3 | 待实现 | 待验收 |
| PS-16-468033d33b | 色彩管理 | 工作色彩空间 sRGB / Display P3 | P0 | 已实现 | M0/M1 已有本机 ICC/位深/导出证据；持续回归 · [M1 色彩与文件闭环](2026-11-03-m1-closure.md) |
| PS-16-4f3db94f33 | 色彩管理 | 读取并应用图片内嵌的 ICC 配置文件 | P0 | 已实现 | M0/M1 已有本机 ICC/位深/导出证据；持续回归 · [M1 色彩与文件闭环](2026-11-03-m1-closure.md) |
| PS-16-b59d8280a5 | 色彩管理 | 导出时嵌入 / 转换配置文件 | P0 | 已实现 | M0/M1 已有本机 ICC/位深/导出证据；持续回归 · [M1 色彩与文件闭环](2026-11-03-m1-closure.md) |
| PS-16-8de06204ac | 色彩管理 | 指定 / 转换配置文件 | P1 | 部分实现 | 内置 sRGB/P3 指定/转换已实现；任意 ICC/CMYK 设置仍待后续 · [M7 配置指定/转换](../research/2026-11-m7-color-progress.md) |
| PS-16-56a0bc682a | 色彩管理 | 软打样 | P2 | 待实现 | 待验收 |
| PS-16-e269e999cb | 色彩管理 | 颜色设置（自定义 CMYK 等） | P2 | 待实现 | 待验收 |
| PS-17-86de52d178 | 面板（未在上面出现的） | 属性 | P0 | 已实现 | 属性面板及工具选项读写当前快照。文字、调整、样式和蒙版由同一注册命令处理。 · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-17-b487bf0eba | 面板（未在上面出现的） | 信息、直方图 | P1 | 待实现 | 待验收 |
| PS-17-01b402a5c6 | 面板（未在上面出现的） | 动作、样式、图层复合、上下文任务栏 | P2 | 待实现 | 待验收 |
| PS-17-48bd1a145f | 面板（未在上面出现的） | 工具预设、测量记录、注释 | P3 | 待实现 | 待验收 |
| PS-17-8368e3ab8e | 面板（未在上面出现的） | 时间轴、库、评论、版本历史 | ✗ | 重新核对 | 待验收 |
| PS-18-5d4f709636 | 自动化 | 脚本 | P0 | 已实现 | 脚本通过本机命令序列执行器，包含读取、尺寸调整、保存与读回示例。 · [2026-11-16 P0 核对](../research/2026-11-p0-closure.md) |
| PS-18-1d67f7829a | 自动化 | 动作：录制 / 回放 / 编辑 | P1 | 待实现 | 待验收 |
| PS-18-c9f7c41baa | 自动化 | 批处理 | P2 | 待实现 | 待验收 |
| PS-18-4f30ead3eb | 自动化 | 插件（类似 Photoshop 的 UXP） | P3 | 待实现 | 待验收 |
| PS-19-21fdf95781 | Adobe 的 AI 功能 | 移除背景 🤖 | P1 | 待实现 | 待验收 |
| PS-19-068a4aa8c2 | Adobe 的 AI 功能 | 生成式填充 🤖 | P2 | 待实现 | 待验收 |
| PS-19-90afa613b4 | Adobe 的 AI 功能 | 生成式扩展 🤖 | P2 | 待实现 | 待验收 |
| PS-19-320004af59 | Adobe 的 AI 功能 | 超分辨率 / 生成式放大 🤖 | P2 | 待实现 | 待验收 |
| PS-19-0af30ae366 | Adobe 的 AI 功能 | 天空替换 🤖 | P2 | 待实现 | 待验收 |
| PS-19-4f4a33e4db | Adobe 的 AI 功能 | 生成图像 / 生成背景 / 生成相似 🤖 | P3 | 待实现 | 待验收 |
| PS-19-6e42c7e07e | Adobe 的 AI 功能 | 协调（合成时自动匹配光照与色彩）🤖 | P3 | 待实现 | 待验收 |
| PS-19-d9de9483c3 | Adobe 的 AI 功能 | 移除干扰物（行人、电线）/ 反射去除 🤖 | P3 | 待实现 | 待验收 |
| PS-19-ef0365d914 | Adobe 的 AI 功能 | Neural Filters：皮肤平滑、智能肖像、上色、超级缩放、JPEG 伪影去除、照片修复、风格转换、妆容迁移、景观混合器、深度模糊、色彩转移 🤖 | P3 | 待实现 | 待验收 |

综合功能条目：265。实现状态：已实现 99；待实现 154；部分实现 3；重新核对 9。

源清单行号可由 --json 查询。该数量是综合条目数，不能当作独立子功能总数或完成百分比。
