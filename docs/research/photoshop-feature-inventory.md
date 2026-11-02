# Photoshop 功能全景与 Strata 优先级

> - 调研日期：2026-11-01，参照 Photoshop 2026（v27.x）的菜单、工具栏、面板结构及 2025–2026 版本更新。
> - 目的：弄清 Photoshop 到底有多少功能，给每个功能定重要程度，作为 Strata 分批实现的依据。
> - 每个大类标题后的 `xxx.*` 是该类在 AI 功能目录里的**草拟命名空间**。这份清单的分类方式，就是 AI 逐层查找功能时看到的目录结构。
> - 2026-11-02 计划修订：以下 P0 是完整 v1.0 范围，不要求 v0.1 预览版一次交齐；功能优先级不变，交付顺序与逐项验收见[路线图](../plans/2026-11-02-strata-v1-roadmap.md)和[P0 验收矩阵](../plans/2026-11-02-p0-acceptance-matrix.md)。

## 规模一览

| 维度 | 数量（约） |
|---|---|
| 工具栏工具 | 70+ |
| 顶部菜单 | 12 个（文件 / 编辑 / 图像 / 图层 / 文字 / 选择 / 滤镜 / 3D / 视图 / 增效工具 / 窗口 / 帮助） |
| 面板 | 40+ |
| 调整（可作调整图层） | 17 种，另有 3 种填充图层 |
| 图层样式 | 10 种 |
| 混合模式 | 27 种（绘画工具另有"背后""清除"） |
| 滤镜 | 约 100 个（其中滤镜库 47 个旧式艺术滤镜） |
| Neural Filters | 约 12 个 |
| 生成式 AI 功能 | 约 10 个 |

## 优先级定义

| 级别 | 含义 | 批次 |
|---|---|---|
| **P0** | v1.0 正式版必须有；v0.1 预览版先交付经过验收的闭环子集。标准由你定：基础修图全部跑通、图层功能全部跑通、常用工具（选区等）、无限画布；人和 AI 都能用 | 第一批完整目标 |
| **P1** | 专业用户日常会用，缺了会被认为"不够专业" | 第二批 |
| **P2** | 进阶与特色功能 | 第三批 |
| **P3** | 低频或旧式功能，最后做或交给社区 | 之后 |
| **✗** | 不做：Adobe 已废弃，或与 Strata 定位无关 | — |

- 🤖 表示需要 AI 模型才能实现（分割、生成、修复等），走"模型插槽"，用户可自行配置模型。
- "人和 AI 都能用"由架构保证（一切皆命令），所以下表不再逐项标注。

---

## 0. Strata 自有基础 `system.*` / `ai.*`

Photoshop 没有、或不对外开放，但 Strata 必须有的部分。

| 功能 | 优先级 | 说明 |
|---|---|---|
| 命令总线 + 功能注册表 | P0 | 一切操作的唯一入口，人、AI、脚本共用 |
| 每个命令自带说明书 | P0 | 用途、参数格式、示例，从代码自动生成；缺说明书的命令 CI 拒绝合并 |
| AI 接口层：浏览分类 / 读取说明 / 调用 / 查看画布与图层 | P0 | 独立于聊天面板，为 MCP 预留 |
| AI 聊天面板 + 模型接入 | P0 | 用户自带模型：OpenAI 兼容接口、Anthropic、本地 Ollama |
| 操作权锁（AI 执行中 / 暂停 / 终止） | P0 | 做在核心层 |
| 统一历史记录（标注来源，AI 回合可整组撤销） | P0 | |
| 无限画布 | P0 | 已明确：无边界内容坐标，画板定义导出范围，画板外像素保留 |
| GPU 渲染管线 + 分块存储 | P0 | 大图流畅；支撑无限画布 |
| Strata 工程文件格式 | P0 | 完整保存图层、蒙版、调整等 |
| 自动保存与崩溃恢复 | P0 | 产品级必备 |
| 中英文界面 | P0 | 我的判断：开源项目面向全球 |
| MCP Server | P1 | 第一期预留口子，第二批接上 |
| 插件系统 | P3 | |

## 1. 文件 `file.*`

| 功能 | Photoshop 名称 | 优先级 | 说明 |
|---|---|---|---|
| 新建文档（尺寸、分辨率、背景、预设） | New | P0 | |
| 打开 PNG / JPEG / WebP / TIFF / HEIC / GIF / BMP | Open | P0 | |
| 最近打开 | Open Recent | P0 | |
| 保存 / 另存为（Strata 格式） | Save / Save As | P0 | |
| 快速导出 / 导出为（格式、尺寸、质量） | Quick Export / Export As | P0 | |
| 置入图片为图层 | Place Embedded | P0 | |
| 恢复到上次保存 | Revert | P1 | |
| PSD 导入 | Open .psd | P1 | 迁移用户的关键；完全兼容很难，先保证图层、蒙版、调整 |
| PSD 导出 | Save as .psd | P1 | |
| 打开 RAW | Camera Raw | P2 | 见第 13 节 |
| 置入链接文件 | Place Linked | P2 | |
| 图层 / 画板导出为文件 | Layers / Artboards to Files | P2 | |
| 批处理、图像处理器 | Batch / Image Processor | P2 | 在 Strata 里可由 AI 或动作驱动 |
| 将文件载入堆栈 | Load Files into Stack | P2 | |
| 文件简介（EXIF / IPTC 元数据） | File Info | P2 | |
| 打印 | Print | P2 | |
| PDF 导入 | Open PDF | P3 | |
| 全景合成、合并到 HDR | Photomerge / Merge to HDR Pro | P3 | |
| 打包、生成图像资源、变量与数据组 | Package / Generate / Variables | P3 | |
| 联系表、快捷批处理、视频帧到图层、旧版存储为 Web | Contact Sheet / Droplet / … | ✗ | |
| 云文档、库、协作项目 | Cloud Docs / Libraries / Projects | ✗ | 依赖 Adobe 云服务 |

## 2. 编辑 `edit.*`

| 功能 | Photoshop 名称 | 优先级 | 说明 |
|---|---|---|---|
| 撤销 / 重做 / 历史记录面板 | Undo / Redo / History | P0 | |
| 历史快照 | Snapshot | P1 | |
| 剪切 / 拷贝 / 合并拷贝 / 粘贴 / 原位粘贴 / 清除 | Cut / Copy / Copy Merged / Paste / Paste in Place / Clear | P0 | |
| 贴入 / 外部粘贴 | Paste Into / Outside | P1 | |
| 填充：颜色 | Fill | P0 | |
| 填充：内容识别 | Fill › Content-Aware | P1 | 见第 7 节 |
| 描边 | Stroke | P1 | |
| 渐隐 | Fade | P2 | |
| 定义画笔 / 图案 / 自定形状 | Define … | P2 | |
| 偏好设置 | Preferences | P0 | |
| 自定义快捷键 / 菜单 / 工具栏 | Keyboard Shortcuts / Menus / Toolbar | P2 | |
| 功能搜索 | Search (⌘F) | P1 | 可与 AI 的功能目录共用同一份数据 |
| 清理缓存 | Purge | P2 | |
| 拼写检查、查找替换文本 | Check Spelling / Find and Replace | P3 | |

## 3. 图像 `image.*`

| 功能 | Photoshop 名称 | 优先级 | 说明 |
|---|---|---|---|
| 图像大小（多种重采样算法） | Image Size | P0 | |
| 画布大小 | Canvas Size | P0 | 修改画板范围；是否删除范围外像素由显式操作决定 |
| 图像旋转 / 翻转 | Image Rotation | P0 | |
| 显示全部 | Reveal All | P0 | |
| 裁切（去透明边） | Trim | P1 | |
| 复制文档 | Duplicate | P1 | |
| 自动色调 / 自动对比度 / 自动颜色 | Auto Tone / Contrast / Color | P0 | |
| RGB 模式 | Mode › RGB | P0 | |
| 灰度模式 | Mode › Grayscale | P1 | |
| CMYK 模式 | Mode › CMYK | P2 | 印刷用户需要，牵涉色彩管理 |
| Lab / 索引色 / 双色调 / 位图 | Lab / Indexed / Duotone / Bitmap | P3 | |
| 多通道 | Multichannel | ✗ | |
| 8 位 / 16 位 | Bits/Channel | P0 | 专业修图需要 16 位，引擎从一开始按高位深设计 |
| 32 位（HDR） | 32 Bits/Channel | P2 | |
| 应用图像 | Apply Image | P2 | |
| 计算 | Calculations | P3 | |
| 分析与测量 | Analysis | P3 | |
| 陷印 | Trap | ✗ | |

## 4. 图层 `layer.*`

| 功能 | Photoshop 名称 | 优先级 | 说明 |
|---|---|---|---|
| 新建图层 / 通过拷贝 / 通过剪切 / 背景转图层 | New Layer / Via Copy / Via Cut | P0 | |
| 复制 / 删除 / 重命名 / 颜色标签 | Duplicate / Delete / Rename | P0 | |
| 图层组（可嵌套）/ 取消编组 | Group / Ungroup | P0 | |
| 显示隐藏 / 锁定（透明像素、像素、位置、全部） | Visibility / Lock | P0 | |
| 不透明度 / 填充不透明度 | Opacity / Fill | P0 | |
| 混合模式（27 种） | Blend Modes | P0 | |
| 排列顺序 / 对齐 / 分布 | Arrange / Align / Distribute | P0 | |
| 向下合并 / 合并可见 / 拼合图像 / 盖印 | Merge Down / Visible / Flatten / Stamp | P0 | |
| 图层蒙版（添加、编辑、停用、应用、反相） | Layer Mask | P0 | |
| 剪贴蒙版 | Clipping Mask | P0 | |
| 调整图层 | Adjustment Layer | P0 | 种类见第 6 节 |
| 填充图层：纯色 / 渐变 | Fill Layer | P0 | |
| 填充图层：图案 | Fill Layer › Pattern | P2 | |
| 栅格化 | Rasterize | P0 | |
| 图层样式：投影、描边、外发光、颜色叠加、渐变叠加 | Layer Style | P0 | 最常用的 5 种 |
| 图层样式：内阴影、内发光、斜面浮雕、光泽、图案叠加；样式预设 | Layer Style | P1 | |
| 链接图层 | Link Layers | P1 | |
| 混合颜色带（Blend If） | Blending Options | P1 | |
| 挖空 | Knockout | P3 | |
| 智能对象：转换、编辑内容、替换内容、栅格化 | Smart Objects | P1 | 我的判断，见文末 |
| 智能滤镜 | Smart Filters | P1 | 依赖滤镜系统 |
| 矢量蒙版 | Vector Mask | P1 | 依赖钢笔与路径 |
| 图层筛选与查找 | Filter / Find Layers | P1 | |
| 画板 | Artboards | P1 | 和无限画布相关 |
| 图层复合 | Layer Comps | P2 | |
| 修边（去边、移除黑 / 白杂边） | Matting | P2 | |
| 视频图层 | Video Layers | ✗ | |

## 5. 选择 `select.*`

| 功能 | Photoshop 名称 | 优先级 | 说明 |
|---|---|---|---|
| 矩形 / 椭圆选框 | Marquee | P0 | |
| 套索 / 多边形套索 | Lasso / Polygonal Lasso | P0 | |
| 魔棒 | Magic Wand | P0 | |
| 快速选择 | Quick Selection | P0 | 传统算法（图割）即可实现 |
| 选择主体 🤖 | Select Subject | P0 | **AI 驱动的关键**：模型画不准套索，局部修图要靠"语义选区"。可接本地分割模型 |
| 全选 / 取消选择 / 重新选择 / 反选 | All / Deselect / Reselect / Inverse | P0 | |
| 选区相加 / 相减 / 交叉 | Add / Subtract / Intersect | P0 | |
| 羽化 / 扩展 / 收缩 / 平滑 / 边界 | Modify | P0 | |
| 变换选区 | Transform Selection | P0 | |
| 磁性套索 | Magnetic Lasso | P1 | |
| 对象选择 🤖 | Object Selection | P1 | |
| 选择天空 🤖 | Select Sky | P1 | |
| 按文字描述选择（如"选中左边的人"）🤖 | — | P1 | Strata 特色，对 AI 尤其有用 |
| 扩大选取 / 选取相似 | Grow / Similar | P1 | |
| 色彩范围 | Color Range | P1 | |
| 快速蒙版 | Quick Mask | P1 | |
| 存储 / 载入选区（Alpha 通道） | Save / Load Selection | P1 | |
| 选择并遮住（细化边缘，如头发） | Select and Mask | P1 | |
| 单行 / 单列选框 | Single Row / Column | P3 | |
| 焦点区域 | Focus Area | P3 | |

## 6. 调整 `adjust.*`

以下都可作为调整图层（非破坏性）。

| 功能 | Photoshop 名称 | 优先级 | 说明 |
|---|---|---|---|
| 亮度 / 对比度 | Brightness/Contrast | P0 | |
| 色阶 | Levels | P0 | |
| 曲线 | Curves | P0 | |
| 曝光度 | Exposure | P0 | |
| 自然饱和度 + 色温 / 色调 | Color & Vibrance | P0 | |
| 色相 / 饱和度 | Hue/Saturation | P0 | |
| 色彩平衡 | Color Balance | P0 | |
| 黑白 | Black & White | P0 | |
| 照片滤镜 | Photo Filter | P0 | |
| 通道混合器 | Channel Mixer | P0 | |
| 反相 / 色调分离 / 阈值 | Invert / Posterize / Threshold | P0 | |
| 渐变映射 | Gradient Map | P0 | |
| 可选颜色 | Selective Color | P0 | |
| 阴影 / 高光 | Shadows/Highlights | P0 | Photoshop 里只能破坏性使用，我们可以做成调整图层 |
| 去色 | Desaturate | P0 | |
| 清晰度 / 去朦胧 / 纹理 | （Camera Raw 中的常用项） | P1 | 做成调整图层，摄影用户需求高 |
| 颜色查找（LUT） | Color Lookup | P1 | |
| 匹配颜色 / 替换颜色 / 色调均化 | Match / Replace Color / Equalize | P1 | |
| 调整预设 | Adjustment Presets | P1 | |
| HDR 色调 | HDR Toning | P3 | |

## 7. 修复与修饰 `retouch.*`

| 功能 | Photoshop 名称 | 优先级 | 说明 |
|---|---|---|---|
| 污点修复画笔 | Spot Healing Brush | P0 | |
| 修复画笔 | Healing Brush | P0 | |
| 仿制图章 | Clone Stamp | P0 | |
| 减淡 / 加深 / 海绵 | Dodge / Burn / Sponge | P0 | |
| 修补工具 | Patch | P1 | |
| 内容识别填充 | Content-Aware Fill | P1 | 传统算法（PatchMatch）可实现 |
| 移除工具 🤖 | Remove Tool | P1 | 先用传统算法，再接修复模型 |
| 红眼工具 | Red Eye | P1 | |
| 模糊 / 锐化 / 涂抹工具 | Blur / Sharpen / Smudge | P1 | |
| 液化 | Liquify | P1 | 人像修图常用 |
| 内容识别移动 | Content-Aware Move | P2 | |
| 仿制源面板 | Clone Source | P2 | |
| 图案图章 | Pattern Stamp | P3 | |

## 8. 绘画 `paint.*`

| 功能 | Photoshop 名称 | 优先级 | 说明 |
|---|---|---|---|
| 画笔（大小、硬度、不透明度、流量、间距、压感） | Brush | P0 | |
| 橡皮擦 | Eraser | P0 | |
| 油漆桶 | Paint Bucket | P0 | |
| 渐变（线性 / 径向 / 角度 / 对称 / 菱形） | Gradient | P0 | |
| 吸管 | Eyedropper | P0 | |
| 拾色器、前景色 / 背景色 | Color Picker | P0 | |
| 颜色面板 | Color | P0 | |
| 颜色取样器 | Color Sampler | P1 | |
| 色板 | Swatches | P1 | |
| 铅笔 | Pencil | P1 | |
| 画笔设置（形状动态、散布、纹理、双重画笔……） | Brush Settings | P1 | |
| 画笔 / 渐变预设管理 | Brushes / Gradients | P1 | |
| 图案预设管理 | Patterns | P2 | |
| ABR 画笔导入 | Import Brushes | P2 | |
| 背景橡皮擦 / 魔术橡皮擦 | Background / Magic Eraser | P2 | |
| 颜色替换 | Color Replacement | P2 | |
| 历史记录画笔 | History Brush | P2 | |
| 混合器画笔 | Mixer Brush | P3 | |
| 历史记录艺术画笔 | Art History Brush | ✗ | |

## 9. 变换与裁剪 `transform.*`

| 功能 | Photoshop 名称 | 优先级 | 说明 |
|---|---|---|---|
| 移动工具（含吸附） | Move | P0 | |
| 自由变换：缩放 / 旋转 / 斜切 / 扭曲 / 透视 / 翻转 | Free Transform | P0 | |
| 裁剪工具（拉直、比例、是否删除裁剪像素） | Crop | P0 | |
| 变形 | Warp | P1 | |
| 透视裁剪 | Perspective Crop | P1 | |
| 操控变形 | Puppet Warp | P2 | |
| 透视变形 | Perspective Warp | P2 | |
| 内容识别缩放 | Content-Aware Scale | P2 | |
| 自动对齐图层 | Auto-Align Layers | P2 | |
| 自动混合图层 | Auto-Blend Layers | P3 | |
| 旋转对象 🤖（2026 新增） | Rotate Object | P3 | |

## 10. 滤镜 `filter.*`

| 功能 | Photoshop 名称 | 优先级 | 说明 |
|---|---|---|---|
| 高斯模糊 | Gaussian Blur | P0 | |
| USM 锐化 | Unsharp Mask | P0 | |
| 添加杂色 | Add Noise | P0 | |
| 智能锐化 | Smart Sharpen | P1 | |
| 高反差保留 | High Pass | P1 | 修图常用（高低频、锐化） |
| 减少杂色 / 中间值 / 蒙尘与划痕 | Reduce Noise / Median / Dust & Scratches | P1 | |
| 动感模糊 / 表面模糊 | Motion / Surface Blur | P1 | |
| 最大值 / 最小值 | Maximum / Minimum | P1 | 常用于收缩、扩展蒙版 |
| 径向模糊 / 镜头模糊 / 方框模糊 / 平均 | Radial / Lens / Box / Average | P2 | |
| 模糊画廊：场景 / 光圈 / 移轴 / 路径 / 旋转 | Blur Gallery | P2 | |
| 镜头校正 | Lens Correction | P2 | |
| 位移 | Offset | P2 | |
| 马赛克 | Mosaic | P2 | |
| 云彩 | Clouds | P2 | |
| 扭曲组：置换 / 挤压 / 极坐标 / 波纹 / 切变 / 球面化 / 旋转扭曲 / 波浪 / 水波 | Distort | P3 | |
| 像素化组其余：彩色半调 / 晶格化 / 点状化 / 碎片 / 铜版雕刻 | Pixelate | P3 | |
| 渲染组：镜头光晕 / 光照效果 / 纤维 / 分层云彩 | Render | P3 | |
| 风格化组：浮雕 / 查找边缘 / 油画 / 风 / 扩散 / 曝光过度 / 拼贴 / 凸出 / 等高线 | Stylize | P3 | |
| 自定 / HSB/HSL | Custom / HSB/HSL | P3 | |
| 自适应广角 / 消失点 | Adaptive Wide Angle / Vanishing Point | P3 | |
| 滤镜库（47 个旧式艺术滤镜） | Filter Gallery | P3 | 适合交给社区 |
| 火焰 / 树 / 图片框 | Flame / Tree / Picture Frame | ✗ | |
| 防抖 | Shake Reduction | ✗ | Adobe 已移除 |

## 11. 文字 `type.*`

| 功能 | Photoshop 名称 | 优先级 | 说明 |
|---|---|---|---|
| 横排文字（点文字 / 段落文字） | Horizontal Type | P0 | |
| 字体、字号、颜色、行距、字距、对齐 | Character / Paragraph | P0 | |
| 栅格化文字 | Rasterize Type | P0 | |
| 直排文字 | Vertical Type | P1 | 中日文用户需要 |
| 文字变形 | Warp Text | P1 | |
| 路径文字 | Type on Path | P2 | |
| 字符样式 / 段落样式 | Character / Paragraph Styles | P2 | |
| OpenType 特性 | OpenType | P2 | |
| 文字转形状 / 创建工作路径 | Convert to Shape / Create Work Path | P2 | |
| 文字蒙版工具 | Type Mask | P2 | |
| 字形面板 | Glyphs | P3 | |
| 匹配字体 🤖 | Match Font | P3 | |

## 12. 矢量与路径 `path.*` / `shape.*`

| 功能 | Photoshop 名称 | 优先级 | 说明 |
|---|---|---|---|
| 形状：矩形（含圆角）/ 椭圆 / 直线 / 多边形 / 三角形 | Shape Tools | P1 | |
| 钢笔 / 弯度钢笔 / 自由钢笔 | Pen / Curvature / Freeform Pen | P1 | |
| 添加 / 删除 / 转换锚点；路径选择 / 直接选择 | Anchor Point / Path Selection | P1 | |
| 路径面板、路径与选区互转 | Paths | P1 | |
| 形状组合（合并 / 减去 / 相交） | Combine Shapes | P2 | |
| 自定形状 | Custom Shape | P2 | |
| SVG 导入导出 | — | P2 | |

## 13. RAW 照片显影 `raw.*`

对应 Photoshop 的 Camera Raw。

| 功能 | 优先级 | 说明 |
|---|---|---|
| RAW 解码（主流相机） | P2 | |
| 基本面板：白平衡、曝光、对比、高光、阴影、白色、黑色、纹理、清晰度、去朦胧 | P2 | 大部分复用第 6 节的调整 |
| 曲线 / 混色器（HSL）/ 颜色分级 | P2 | |
| 细节：锐化、降噪 | P2 | |
| 效果：颗粒、晕影 | P2 | |
| 预设 | P2 | |
| 光学：镜头配置文件、色差、暗角 | P3 | |
| 几何：Upright 透视校正 | P3 | |
| AI 蒙版（主体 / 天空 / 人物）🤖、AI 降噪 🤖、镜头模糊 🤖 | P3 | |

## 14. 视图与导航 `view.*`

| 功能 | Photoshop 名称 | 优先级 | 说明 |
|---|---|---|---|
| 缩放 / 抓手 / 适合屏幕 / 100% | Zoom / Hand / Fit / Actual | P0 | |
| 多文档标签页 | Tabs | P0 | |
| 标尺 / 参考线 / 对齐吸附 | Rulers / Guides / Snap | P0 | |
| 旋转视图 | Rotate View | P1 | |
| 智能参考线 | Smart Guides | P1 | |
| 网格 / 像素网格 | Grid / Pixel Grid | P1 | |
| 导航器 | Navigator | P1 | |
| 多窗口排列 / 屏幕模式 | Arrange / Screen Mode | P2 | |
| 校样颜色 / 色域警告 | Proof Colors / Gamut Warning | P2 | |
| 新建参考线版面 | New Guide Layout | P3 | |

## 15. 通道 `channel.*`

| 功能 | 优先级 | 说明 |
|---|---|---|
| 查看单个颜色通道 | P1 | |
| Alpha 通道（存储选区） | P1 | |
| 复制通道、通道转选区 | P1 | |
| 专色通道 | P3 | |

## 16. 色彩管理 `color.*`

| 功能 | 优先级 | 说明 |
|---|---|---|
| 工作色彩空间 sRGB / Display P3 | P0 | Mac 屏幕是 P3 广色域 |
| 读取并应用图片内嵌的 ICC 配置文件 | P0 | |
| 导出时嵌入 / 转换配置文件 | P0 | |
| 指定 / 转换配置文件 | P1 | |
| 软打样 | P2 | |
| 颜色设置（自定义 CMYK 等） | P2 | |

## 17. 面板（未在上面出现的）

| 面板 | 优先级 |
|---|---|
| 属性 | P0 |
| 信息、直方图 | P1 |
| 动作、样式、图层复合、上下文任务栏 | P2 |
| 工具预设、测量记录、注释 | P3 |
| 时间轴、库、评论、版本历史 | ✗ |

## 18. 自动化 `action.*`

| 功能 | 优先级 | 说明 |
|---|---|---|
| 脚本 | P0 | 由 Strata 的命令接口统一承担，即第 0 节 |
| 动作：录制 / 回放 / 编辑 | P1 | 有了命令总线，录制动作几乎是顺带的 |
| 批处理 | P2 | |
| 插件（类似 Photoshop 的 UXP） | P3 | |

## 19. Adobe 的 AI 功能 `gen.*` / `neural.*`

在 Strata 里全部走"模型插槽"。

| 功能 | 优先级 | 说明 |
|---|---|---|
| 移除背景 🤖 | P1 | 基于"选择主体"，几乎是顺带的 |
| 生成式填充 🤖 | P2 | |
| 生成式扩展 🤖 | P2 | 和无限画布天然契合 |
| 超分辨率 / 生成式放大 🤖 | P2 | |
| 天空替换 🤖 | P2 | |
| 生成图像 / 生成背景 / 生成相似 🤖 | P3 | |
| 协调（合成时自动匹配光照与色彩）🤖 | P3 | |
| 移除干扰物（行人、电线）/ 反射去除 🤖 | P3 | |
| Neural Filters：皮肤平滑、智能肖像、上色、超级缩放、JPEG 伪影去除、照片修复、风格转换、妆容迁移、景观混合器、深度模糊、色彩转移 🤖 | P3 | |
| AI Assistant（Adobe 2026 年的聊天助手，测试版） | — | 这正是 Strata 的核心，见第 0 节 |

## 20. 明确不做

- 3D（Adobe 已于 2023 年移除）
- 视频图层、视频时间轴（帧动画 GIF 以后可考虑）
- 依赖 Adobe 云服务的功能：云文档、库、协作项目、Firefly Boards
- 旧版"存储为 Web 所用格式"、快捷批处理、联系表

---

## 第一批（P0）范围汇总

- **地基**：命令总线、AI 接口层、AI 聊天面板与模型接入、操作权锁、统一历史、无限画布、GPU 渲染、工程文件、自动保存、色彩管理基础
- **文件**：新建、打开常见格式、保存、导出、置入
- **图层**：除智能对象、矢量蒙版、画板以外的全部常用图层功能，包括蒙版、剪贴蒙版、调整图层、混合模式、5 种常用图层样式
- **选区**：选框、套索、魔棒、快速选择、选择主体 🤖、选区运算与修改
- **调色**：17 种调整全部可作调整图层，另有自动色调
- **修图**：污点修复、修复画笔、仿制图章、减淡加深海绵、高斯模糊、USM 锐化、添加杂色
- **绘画**：画笔、橡皮擦、油漆桶、渐变、吸管、拾色器
- **变换**：移动、自由变换、裁剪、图像大小、画布大小、旋转翻转
- **文字**：横排点文字与段落文字的基础排版
- **视图**：缩放、抓手、标尺、参考线、吸附、多文档标签

## 我替你做的判断（请重点看）

1. **选择主体列为 P0**（唯一进入第一批的 🤖 功能）：AI 没法像人一样精确地画套索，没有语义选区，AI 只能做全图调整，做不了"把人物提亮、背景压暗"这类局部修图。
2. **基础文字列为 P0**：产品级图像编辑器没有文字会显得残缺；直排、文字变形等放到 P1。
3. **智能对象、智能滤镜、矢量形状、钢笔放到 P1**：你说"图层功能全部跑通"，但这几项各自牵涉一个大子系统（非破坏性滤镜管线、矢量引擎）。放进第一批会大幅推迟首发。
4. **16 位色深列为 P0**：后补高位深要动整个引擎，不如一开始就做。
5. **PSD 导入导出放 P1**：对吸引 Photoshop 用户很重要，但完整兼容的工作量很大，第一批先用自有格式。
6. **液化、内容识别填充放 P1**：常用，但第一批已经很满。
