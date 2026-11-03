# M1a 工程格式与命令契约

本文件记录 2026-11-03 实现的内部 version 0；不承诺 v1 正式工程兼容性。源数据为 straight-alpha RGBA8/RGBA16，显示缓存不能作为保存输入。

## 工程容器

`.strata` 是单文件 SQLite，`application_id = 0x53545241`，`user_version = 0`。`metadata` 表只有 id=1，保存 JSON 与 SHA-256；`tiles` 主键为 `(layer,x,y)`，保存 depth、data 与 SHA-256。256×256 固定块，16 位样本固定小端序。缺块为 RGBA 全零，透明非零 RGB 仍需保存。

元数据保存文档/内容/图层稳定 ID、画板位置/尺寸/名称、图层顺序/位置/显隐/不透明度/混合模式、活动图层、源 ICC 与工作 ICC、回退信息、整数 `resolution_dpi` 与可选预设标识。旧工程缺少 DPI 时按 72 读取，预设缺省为自定义；旧版读取器拒绝新增字段，内部 v0 不承诺前向兼容。会话 ID、运行 revision、历史、控制权和能力句柄不写入工程。

读取先验证签名、版本、数据库完整性、元数据与像素额度，再验证摘要、位深、ID 唯一性、引用、坐标和属性。M1b 更新后，单文档源块逻辑限 16 GiB，图层最多 128，元数据限 4 MiB，块记录最多 65536（还受总字节限制）。工程命令使用逐块读写，内存形式的 `ProjectArchive` 读出辅助接口仍限 512 MiB。画板、图片解码和平面导出各限 512 MiB，并受全局资源预算约束。

首次保存与另存为在同目录创建临时工程，事务完成后重新读取验证、同步，再原子发布。已保存工程的普通保存更新变化块的摘要/数据，SQLite 使用 DELETE rollback journal、FULL synchronous、fullfsync；正常完成后没有常驻 WAL 边车。崩溃留下的 rollback journal 在私有临时副本中恢复，打开原文件不会擅自写回。保存始终绑定固定内容状态；期间新编辑不会被错误标记为已保存。

发布前失败保留旧文件。已有目标的事务已提交，或新目标已原子发布后的同步失败返回 `PublishedUncertain`，保留脏状态，并在文件记录中显示持久性不确定；仅临时数据库的事务提交不算发布。进程崩溃与故障注入测试不能代替物理断电测试。M1b 已接入独立恢复副本与磁盘块后备，详见下文。

PNG/TIFF 导出保持文档位深并嵌入工作 ICC；未编辑单层精确保留源样本（包括透明 RGB）。编辑导出使用线性 Display P3 浮点转换、f64 alpha/混合计算和原位深量化。无隐式 16→8 位降级。写出后重解码比较像素和 ICC，验证成功才发布。TIFF ICC 使用独立且有上限的元数据读取，避免 image 0.25.10 对小图片静默丢失 profile。

M1b 导出扩展：`format` 可选 `png/tiff/jpeg/webp`。WebP 默认无损；`webp_lossless:false` 启用有损 RGB，`quality` 为 1–100（默认 80），alpha 保持无损。JPEG 质量 1–100（默认 90），必须提供输出色域下 8 位 RGB `background`，在线性光合成透明区。`background` 只适用于 JPEG；`quality` 只适用于 JPEG 或有损 WebP；`webp_lossless` 只适用于 WebP。16 位导出 JPEG/WebP 必须显式 `allow_quantization:true`。PNG/TIFF 保持位深。

`target_space` 可省略以保留工作 ICC，或设 `srgb` / `display_p3`。可选 `width/height` 必须同时提供正整数，省略或相同尺寸则保持原尺寸路径；WebP 宽高最多 16383。缩小用线性光面积滤波，放大用像素中心双线性滤波，均按预乘 alpha 过滤，浮点数据按行处理。全部输出写入文档 DPI：PNG pHYs、JPEG JFIF/EXIF、TIFF 有理数密度、WebP EXIF。有效等向输入 DPI 读取为整数，未知/不支持密度回退 72；分辨率不更改源像素。临时文件验证尺寸、位深、ICC、DPI 和 alpha/无损样本后原子发布；有损 RGB 不进行样本相等判定。导出不改变文档样本、历史、工作空间、DPI 或脏状态。

`file.new` 接受显式宽高，或 `web_1080p/a4_300dpi/photo_4x6_300dpi/square_2048` 预设；同时给出的宽高须与预设一致。`bit_depth` 为 8/16，DPI 为 1–2400 整数（默认 72 或预设值），工作空间为 sRGB/P3。背景 RGBA 按文档位深解释，默认稀疏透明；修改后的参数、颜色和预设信息随工程保存。

## 命令执行

`cargo run -p strata-devtool -- catalog` 输出从类型生成的参数/结果 JSON Schema、双语标题、英文说明、权限、副作用和隔离示例；`make commands` 执行 28 个注册命令的隔离示例，并检查自动生成的 [命令参考](../commands/reference.md) 是否与注册表一致。`strata-devtool docs OUT.md` 可重新生成。目录是源码真值，schema 不手工复制。

| 类别 | 命令 |
|---|---|
| 文档 | `file.new`、`file.open`、`file.recover`、`document.inspect` |
| 图层 | `layer.list/create/delete/duplicate/rename/set_visibility/set_opacity/set_blend_mode/set_position/move_order/select` |
| 事务与历史 | `document.batch`、`edit.undo/redo/undo_turn` |
| 视图 | `view.pan/zoom/fit` |
| 文件 | `file.save/save_as/export/place`、`file.recent.list` |
| 工作空间 | `color.convert`（sRGB、Display P3） |

来源 User/AI/Test、会话、文档、revision 和 call ID 由宿主绑定，不能通过 JSON 冒充。执行器先验证控制权与参数，创建候选；宿主准备显示资源成功后再次检查 revision/epoch，才提交核心状态与画面。事件在锁外送达。停止回合立即撤销提交资格；暂停只允许明确白名单的用户编辑。call ID 重试返回原结果，同 ID 不同参数冲突；提交时再次检查，覆盖同时准备的重试。普通收据保留一小时，视图收据 30 秒，最多 4096 条。

文件参数只接受宿主生成的能力 token。能力绑定来源、目标、时效、文件大小/mtime（Unix 还绑定 device/inode）；覆盖权限由原生面板或开发工具显式提供。写能力单次使用，失败后也需重新授权。不能把这些校验当作对同机恶意进程的完整沙箱；更细的外发/模型权限属于 M2。

M1b 首批新增 `file.place`：资源为已授权的图片，`x/y` 默认为 0，在活动图层上方插入新像素层，不调整画板或缩放图片。8→16 位先升位后转换 ICC，16→8 位先转换 ICC 再按全量程量化（含 alpha）；当前文档位深和工作 ICC 不变。准备后、提交前均复核资源；失败不增加历史。与 `file.open` 不同，置入只增加一条编辑历史；与其他文件动作一样不允许批事务。`file.recent.list` 为独立只读查询、不可批处理，可在无文档时调用：User/Test 返回宿主提供的最多 10 项历史；AI 仅返回此编辑会话内、本回合已有且仍有效的读取授权条目，隐藏路径，只引用已存在的读取句柄。列表和可用性元数据不授予任何文件能力。原生菜单同样调用此查询，选择条目后仍重新授权。

批事务预先排除文件、视图、历史和生命周期命令；中途失败不提交。稳定结果引用示例：

```json
{"steps":[
  {"command":"layer.create","params":{"name":"Overlay"}},
  {"command":"layer.set_opacity","params":{"layer":{"id":{"$ref":{"step":0,"field":"layer_id"}}},"opacity":0.5}}
]}
```

历史最多 64 条，历史/当前候选引用的独立像素块逻辑额度 24 GiB；源像素 COW，旧版本和当前版本共用同一进程资源池。默认 CPU 像素/CPU 缓存/暂存预算合计 2 GiB，GPU 资源估算 512 MiB，临时块文件配额 32 GiB。优先淘汰可重建缓存，再将未被读取者固定的源/历史块写入同一个匿名临时文件；回载校验 SHA-256，错误向调用方报告，不以零块替代。额度不足时拒绝候选，文档/历史不提交。计数器覆盖托管资源，另有元数据、系统编解码器、GPU 驱动及原生 UI 开销，不能据此声称进程 RSS 硬上限。拖动只更新预览，松手提交一次，Esc 或新 revision/控制权变化取消预览。文件执行记录独立于撤销栈。活动图层属于视图选择，不新增编辑历史。

## HEIC 与恢复副本

`strata-platform` 在 macOS 用 ImageIO 解码 HEIC 主图；核心通过受信宿主的 `PlatformDecoder` 接口调用。portable crates 不链接平台 API。图像按 RGB ICC、8/16 位整数输出及 EXIF 方向规范化，打开和置入走同一资源检查。原生样例验证了 8 位 P3 和整数 RGB101010 主图；10 位全量程提升为 Rgba16。适配器直接读取整数解码数据，避免 8 位显示上下文的预乘舍入；16 位相邻码值和透明 RGB、8 位低 alpha 均有保真测试。浮点/HDR/深度辅助图不纳入当前整数文档能力。

宿主将恢复目录绑定到 Application Support/Strata/Recovery；目录私有，文件名为宿主会话 UUID。恢复工程的元数据外包一层 `id/recovery/document`，正文工程格式不变。首份副本或更换文档时原子发布；同一文档后续检查点用 FULL-sync SQLite 事务更新变化块。启动发现热 rollback journal 时，在私有副本中恢复并读取，保留源文件。

原生计时器在脏文档 30 秒空闲后触发，1 秒轮询，忙碌时顺延；成功只更新恢复时间，不清脏状态、不改变历史和保存路径。发布/事务提交前失败显示状态并保留上次完整副本；已提交后的同步错误显示未确认，不声称旧文件未变。`file.recover` 接受读取资源与 `discard_changes`，校验全部块并准备画布后提交为未保存文档；`original_path` 仅作记录，不构成文件能力。恢复副本在新副本或用户保存成功之前保留；正常关闭/退出经保存或丢弃决策后清理，退出等待清理而不把长磁盘 IO 放进主线程。

## 可重现开发验收

```sh
make commands
cargo run --release -p strata-devtool -- demo build/m1a-demo-new
cargo run --release -p strata-devtool -- capacity
cargo run --release -p strata-devtool -- memory-stress build/new-memory-stress
cargo run --release -p strata-devtool -- verify-exports build/example.strata build/example.png build/example.tiff
```

`demo` 拒绝已有目录，生成 PNG/TIFF×8/16 位输入，执行复制、属性/位置修改、撤销/重做、工程往返以及两种导出格式，自动比较元数据、ID、ICC 与像素。`capacity` 尝试 20 个独立 6000×4000 填色层，记录每层耗时与实际接纳/拒绝点，不用共享副本伪造独立层。

`verify-exports` 只读打开工程和指定输出文件，比较尺寸、ICC 和全部导出样本，用于复核原生界面产生的真实文件。

`run SCRIPT.json` 用于明确资源绑定的自定义命令。文件路径只在宿主 resources 中出现：

```json
{"resources":[{"name":"result","path":"build/example.strata","write":true}],"commands":[
  {"command":"file.new","params":{"width":640,"height":480,"bit_depth":16}},
  {"command":"layer.create","params":{"name":"Color","fill":[32768,32769,50001,65535]}},
  {"command":"file.save_as","params":{"resource":"result"}}
]}
```

M1a 新直接依赖的锁定版本见 Cargo.lock：serde/serde_json、schemars、uuid、sha2 为 MIT/Apache-2.0 双许可，rusqlite 为 MIT，bundled SQLite 为 public domain，tiff 为 MIT。第三方正式分发清单仍在发布前生成。

M1b 新增编码绑定 `webp 0.3.1`（MIT/Apache-2.0）、`libwebp-sys 0.9.6`（MIT）及其 bundled libwebp（BSD-3-Clause）；`png 0.18.1` 与 `libc` 已锁定。使用现有 C 工具链编译，核心不依赖系统安装的 libwebp。
