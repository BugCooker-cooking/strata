# M1a 工程格式与命令契约

本文件记录 2026-11-03 实现的内部 version 0；不承诺 v1 正式工程兼容性。源数据为 straight-alpha RGBA8/RGBA16，显示缓存不能作为保存输入。

## 工程容器

`.strata` 是单文件 SQLite，`application_id = 0x53545241`，`user_version = 0`。`metadata` 表只有 id=1，保存 JSON 与 SHA-256；`tiles` 主键为 `(layer,x,y)`，保存 depth、data 与 SHA-256。256×256 固定块，16 位样本固定小端序。缺块为 RGBA 全零，透明非零 RGB 仍需保存。

元数据保存文档/内容/图层稳定 ID、画板位置/尺寸/名称、图层顺序/位置/显隐/不透明度/混合模式、活动图层、源 ICC 与工作 ICC、回退信息。会话 ID、运行 revision、历史、控制权和能力句柄不写入工程。

读取先验证签名、版本、数据库完整性、元数据与像素额度，再验证摘要、位深、ID 唯一性、引用、坐标和属性。当前源块总量限 512 MiB，图层最多 128，元数据限 4 MiB，块记录最多 2048。解码、导出和历史快照各有额外临时内存；这不是整个进程的 512 MiB 上限。

首次保存与另存为在同目录创建临时工程，事务完成后重新读取验证、同步，再原子发布。已保存工程的普通保存更新变化块的摘要/数据，SQLite 使用 DELETE rollback journal、FULL synchronous、fullfsync；正常完成后没有常驻 WAL 边车。崩溃留下的 rollback journal 在私有临时副本中恢复，打开原文件不会擅自写回。保存始终绑定固定内容状态；期间新编辑不会被错误标记为已保存。

发布前失败保留旧文件。已有目标的事务已提交，或新目标已原子发布后的同步失败返回 `PublishedUncertain`，保留脏状态，并在文件记录中显示持久性不确定；仅临时数据库的事务提交不算发布。进程崩溃与故障注入测试不能代替物理断电测试。30 秒检查点、恢复 UI、磁盘历史后备和全局内存预算属于 M1b。

PNG/TIFF 导出保持文档位深并嵌入工作 ICC；未编辑单层精确保留源样本（包括透明 RGB）。编辑导出使用线性 Display P3 浮点转换、f64 alpha/混合计算和原位深量化。无隐式 16→8 位降级。写出后重解码比较像素和 ICC，验证成功才发布。TIFF ICC 使用独立且有上限的元数据读取，避免 image 0.25.10 对小图片静默丢失 profile。

M1b 导出扩展：`format` 还可选 `jpeg`、`webp`（无损）。`target_space` 可省略以保留工作 ICC，或为 `srgb` / `display_p3`。JPEG 的 `quality` 范围 1–100（默认 90），必须提供输出色域下 8 位 RGB `background`，透明区域在线性光下合成；非 JPEG 不接受这两个参数。16 位文档导出 JPEG/WebP 必须设置 `allow_quantization:true`，先转换色域再进行全量程量化。PNG/TIFF 不降位；所有导出不改变文档样本、工作色域、历史或脏状态。无损输出逐像素复核；JPEG 为有损，仅验证可重解码、尺寸、8 位不透明输出及 ICC，不声称样本无损。临时文件验证成功后仍使用原有同步/原子发布路径。尚无尺寸缩放或有损 WebP。

## 命令执行

`cargo run -p strata-devtool -- catalog` 输出从类型生成的参数/结果 JSON Schema、双语标题、英文说明、权限、副作用和隔离示例；`make commands` 执行所有注册示例。目录是源码真值，文档不手工复制 schema。

| 类别 | 命令 |
|---|---|
| 文档 | `file.new`、`file.open`、`document.inspect` |
| 图层 | `layer.list/create/delete/duplicate/rename/set_visibility/set_opacity/set_blend_mode/set_position/move_order/select` |
| 事务与历史 | `document.batch`、`edit.undo/redo/undo_turn` |
| 视图 | `view.pan/zoom/fit` |
| 文件 | `file.save/save_as/export/place` |
| 工作空间 | `color.convert`（sRGB、Display P3） |

来源 User/AI/Test、会话、文档、revision 和 call ID 由宿主绑定，不能通过 JSON 冒充。执行器先验证控制权与参数，创建候选；宿主准备显示资源成功后再次检查 revision/epoch，才提交核心状态与画面。事件在锁外送达。停止回合立即撤销提交资格；暂停只允许明确白名单的用户编辑。call ID 重试返回原结果，同 ID 不同参数冲突；提交时再次检查，覆盖同时准备的重试。普通收据保留一小时，视图收据 30 秒，最多 4096 条。

文件参数只接受宿主生成的能力 token。能力绑定来源、目标、时效、文件大小/mtime（Unix 还绑定 device/inode）；覆盖权限由原生面板或开发工具显式提供。写能力单次使用，失败后也需重新授权。不能把这些校验当作对同机恶意进程的完整沙箱；更细的外发/模型权限属于 M2。

M1b 首批新增 `file.place`：资源为已授权的图片，`x/y` 默认为 0，在活动图层上方插入新像素层，不调整画板或缩放图片。8→16 位先升位后转换 ICC，16→8 位先转换 ICC 再按全量程量化（含 alpha）；当前文档位深和工作 ICC 不变。准备后、提交前均复核资源；失败不增加历史。与 `file.open` 不同，置入只增加一条编辑历史；与其他文件动作一样不允许批事务。最近文件目前只在原生宿主菜单暴露，没有注册空壳 `file.recent.list`。

批事务预先排除文件、视图、历史和生命周期命令；中途失败不提交。稳定结果引用示例：

```json
{"steps":[
  {"command":"layer.create","params":{"name":"Overlay"}},
  {"command":"layer.set_opacity","params":{"layer":{"id":{"$ref":{"step":0,"field":"layer_id"}}},"opacity":0.5}}
]}
```

历史最多 64 条，历史引用的独立像素块预算 768 MiB；源像素 COW。拖动只更新预览，松手提交一次，Esc 或新 revision/控制权变化取消预览。文件执行记录独立于撤销栈。活动图层属于视图选择，不新增编辑历史。

## 可重现开发验收

```sh
make commands
cargo run --release -p strata-devtool -- demo build/m1a-demo-new
cargo run --release -p strata-devtool -- capacity
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
