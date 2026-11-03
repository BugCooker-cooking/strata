# M1b 第二批：导出格式与参数

2026-11-03。在工作区提交 `e68affa` 双平台 CI 与干净源码构建成功后，继续推进文件计划。本批不宣称完整 M1b 已完成。

## 交付

- `file.export` 新增 JPEG 和无损 WebP，与原有 PNG/TIFF 共用授权、固定快照、临时文件复核、原子发布和执行记录路径；命令数量仍为 26。
- 四种格式均可保留文档工作 ICC，或只对输出副本转换为 sRGB / Display P3。PNG/TIFF 保持文档位深；WebP/JPEG 仅 8 位，16 位输入必须显式 `allow_quantization:true`。
- JPEG 支持质量 1–100（默认 90），必须提供输出色域下的 8 位 RGB 背景；透明区在线性光下合成，以有界条带处理，避免额外整图浮点副本。原生界面提供白色/黑色，命令可指定任意 RGB。
- 无损 WebP 保留透明度及透明 RGB；不提供虚假的有损质量滑杆。质量/背景参数只适用于 JPEG，其他格式传入则拒绝。
- 原生保存面板显示格式、输出色域、JPEG 质量/背景及 16→8 位确认；格式变化同步扩展名；非法输入在面板内拦截，不先写文件。按 Emil 界面指南明确参数适用性和风险提示，不添加高频操作动画。

## 验收

- `STRATA_REQUIRE_GPU=1 make test`：114 项 Rust 测试（含 doc-test）、16 项 XCTest 全部通过；本机 GPU 必须存在。
- `make lint`、`make commands` 和 Release 构建通过。注册表为 PNG/TIFF/JPEG/WebP 分别提供隔离示例。
- Rust 测试验证 WebP 全部样本（含透明 RGB）/ICC 精确往返、JPEG 质量影响编码、JPEG 尺寸/深度/ICC、背景线性合成、非法质量/透明输入/16 位输入不覆盖目标、工作色域转换结果以及源文档样本/历史/脏状态不变。
- XCTest 验证扩展名同步、16 位量化默认关闭且必须确认、PNG/TIFF 不量化、质量范围/非整数/无效文本拒绝、WebP 不带 JPEG 参数。
- 原生 Release：质量 101 被阻止，改为 85 后成功导出 `build/export-options-ui-20261103.jpg`，系统 ImageIO 读取为 JPEG、1024×1536、Display P3。
- 原生无损 WebP `build/export-options-ui-20261103.webp` 与保存工程逐像素、尺寸、ICC 一致；证据 [native-webp](data/2026-11-03-export-native-webp.json)。导出后源工程仍无未保存修改。
- 原生 16 位工程切换 WebP：未勾选时报错，勾选后生成 `build/m1a-demo-20261103/quantization-ui-20261103.webp`，系统读取为 8 位 RGBA/sRGB；源工程仍为 16 位、无未保存修改。
- 更新构建前保留用户窗口的新调整到 `build/workspace-user-preserved-20261103-1515.strata`，没有覆盖此前验收工程。

JPEG 的校验不比较有损像素相等；不能把成功解码或 ICC 嵌入当作无损证明。编码使用现有锁定依赖，没有新增服务、联网或第三方库。画布渲染未变，本轮沿用工作区性能门槛记录，不重复宣称性能优化。

## 待续

HEIC、尺寸缩放、新建文档参数/预设、`file.recent.list` 的宿主可见范围、30 秒空闲恢复副本与启动恢复入口、全局 CPU/GPU/历史预算和磁盘后备。当前只支持无损 WebP，有损 WebP 尚未实现。M0 额外硬件验收缺口继续保留。
