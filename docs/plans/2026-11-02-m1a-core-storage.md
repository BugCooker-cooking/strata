# M1a 命令、分块与工程往返实施计划

状态：2026-11-03，Task 1–8 已完成；当前 Mac 集成与性能、提交后干净构建和远程 Linux/macOS CI 均通过。发起人确认先使用当前 Mac，其他设备暂缺；macOS 14、跨屏、真实触控板与标准机复验继续保留，不开放产品预览。

依据：[路线图 M1](2026-11-02-strata-v1-roadmap.md#4-m1-核心引擎与安全存储)、[总体设计 §6](../design/2026-11-02-strata-v1-design.md#6-核心子系统)、[P0 验收矩阵](2026-11-02-p0-acceptance-matrix.md)。执行者为当前项目开发者；代码评审与测试结果随每步提交记录，不把计划中的测试名当作已存在的测试。

## 交付边界

M1a 的内部检查点：打开 PNG/TIFF 8/16 位 → 建立单画板和像素图层 → 增删/移动/显隐/不透明度/四种混合模式 → 撤销重做 → 保存 `.strata` → 重开 → 导出 PNG/TIFF。AppKit 与无界面开发工具使用同一个命令执行器。无编辑往返逐样本相同；编辑后导出有独立高精度参考。

M1b 继续完成完整文件格式与参数、最近文件、恢复入口、30 秒空闲检查点、全局缓存/历史磁盘后备及 50MP×30 独立 16 位图层压力验收。M1a 的容量限制不是全局内存预算，也不代表超内存工作集已达标。M0 已支持的图片解码和只读预览不应因迁移被悄悄移除。

## 实施顺序与共享约定

任务依赖：1 → 2 → 3 → 4；5 依赖 2/3，6 依赖 2/4，7 依赖 3/5/6；8 集成所有任务。按此顺序逐项提交；先验证失败用例，再实现；不一次创建未用到的 crate。

- 源像素仍为 straight-alpha `Rgba8/Rgba16`，工作空间的 ICC 与位深显式存储；显示缓存不得成为工程保存或导出的输入。
- 256×256 固定分块，世界坐标使用有符号整数；负坐标用欧几里得除法。缺失块代表全部 RGBA 为零。alpha 为零但 RGB 非零的源数据不能静默丢弃。
- 元数据快照可复制，像素块用共享不可变引用；写入只复制受影响块。全局预算、磁盘后备和历史配额仍需后续实现，不能用 COW 当作内存方案已完成。
- 文档稳定 ID 与运行中的会话 ID 分开。撤销后重新修改，内容状态可回到历史版本，但提交 revision 单调递增；禁止用撤销降低 revision。
- 文档可变接口限制在核心命令模块内。外壳只读 snapshot/dispatch；已移除公开 `Engine::set_document`。FFI `open_image` 仅为宿主兼容包装，内部授予资源并执行注册的 `file.open`，不能绕过命令。
- 文件读取/写入使用宿主授予的资源句柄；命令 JSON 不能用任意路径或自报来源取得权限。`Test` 来源只在隔离测试/开发工具中由宿主构造。

## Task 1：分块坐标、稀疏像素与 COW

修改 `crates/strata-pixels/src/lib.rs`，新增 `src/tiles.rs` 和 `tests/tiles.rs`。

- [x] 提供 `TileCoord`、`PixelValue`、`TiledPixels`；坐标构造和像素访问不依赖 GPU、Swift 或文档命令。
- [x] 明确 8/16 位格式，写错格式在改变状态之前拒绝；相邻 32768/32769 与透明非零 RGB 精确保留。
- [x] 空区域零分配；清空最后一个非零样本可移除块。克隆共享块，写入不改变历史快照。
- [x] 每个实例设显式最大块数，超过限制返回错误并保持原状态；记录该限制只约束当前实例引用的块数，不计所有历史快照的总内存。
- [x] 覆盖 -257/-256/-1/0/255/256、i64 端点、跨块读写、格式错误、限额、COW 和无操作写入。

验收：`cargo test -p strata-pixels --locked`、工作区 clippy；映射 SYS-07/08、IMG-07 的基础部分。此步不接入显示，不宣称图层或保存可用。

## Task 2：文档/图层模型与导入候选

拆分 `crates/strata-core/src/lib.rs`，新增 `document.rs`、`layer.rs`、`ids.rs`；在 io/color 增加可复用的候选构建接口。

- [x] 单画板、按顺序排列的像素图层、稳定 ID、名称、显隐、不透明度、混合模式、平移位置、文档格式/ICC、活动图层。
- [x] ID 不因删除/撤销被复用；名称不唯一时只返回候选，不能猜目标。序列化快照使用独立 DTO，外部反序列化仍须验证所有不变量。
- [x] 初期导入保留有效 RGB 源 ICC 及源样本；未标记输入按 sRGB。用户显式选择不同工作空间才进行同位深转换，并保留导入来源信息。损坏 ICC 的回退状态可见。
- [x] 单一 `DocumentCandidate` 先验证尺寸/限额/ICC/分块，再交给执行器提交。旧文档、历史与旧画面在候选失败时不变。
- [x] 文档“内容状态标识”和“已保存状态标识”用于脏状态；revision 单独防止迟到提交。撤销回到已保存状态可恢复干净标记。

测试目标：`document_ids_survive_snapshot`、`layer_ids_are_not_reused`、`duplicate_names_require_id`、`import_preserves_16bit_and_hidden_rgb`、`invalid_candidate_keeps_document`。

映射 FILE-01/02、LAY-01/02/04/05/06/07、IMG-06/07 的 M1a 子集。

## Task 3：命令注册表、来源与提交协议

在 core 新增 `commands/{mod,registry,executor,context,error}.rs`。首次需要时增加 serde/JSON Schema 依赖，核对官方 API、许可证与实际版本并锁定 Cargo.lock；不在本计划猜测版本。

- [x] 从参数/返回类型生成 schema；元数据含中英文标题、英文说明、前置条件、副作用、权限、batchable、示例。用声明式宏统一注册；如后续确需属性宏再单独建 proc-macro crate。
- [x] `query/mutate/view/external/lifecycle/history` 分类实现；来源由宿主绑定。执行顺序为授权/控制权 → 参数/前置条件 → revision → 候选准备 → 再检查提交资格 → 原子交换 → 记录 → 锁外事件。
- [x] `document_id/session_id/turn_id/call_id` 明确作用域；同一 call_id+参数重试返回原结果，不重复提交；同 ID 不同参数报冲突。去重记录设容量和过期策略，活动调用不可提前淘汰。
- [x] 实现应用范围控制权状态；用测试回合验证 AI 运行时用户 mutate 被拒绝，暂停时只放行明确白名单。停止先撤销提交资格，迟到候选不得提交。
- [x] 第一批注册 `document.inspect`、`layer.list`、图层基础属性命令及 `view.pan/zoom/fit`。文件/历史命令在对应任务完成时加入；空壳命令不对目录暴露。
- [x] Rust 可见性与编译失败用例证明外部不能改文档；不能仅靠 UI 按钮置灰。

测试目标：`locked_mutation_is_rejected`、`stale_revision_cannot_commit`、`stopped_turn_cannot_commit`、`call_retry_commits_once`、`changed_call_payload_is_rejected`、`listener_can_query_after_commit`。

映射 SYS-01/02/05；外发/模型权限在 M2 使用前继续扩展，文件能力最小授权在 Task 6 先完成。

## Task 4：线性历史、批事务与交互会话

新增 core `history.rs`、`commands/batch.rs`、`interaction.rs`。

- [x] 历史保留前后内容快照或差分，像素共享块；新修改清除 redo 分支。undo/redo 只移动历史并增加 revision，不再生成普通编辑条目。
- [x] AI 回合分组只撤销尾部连续回合；后续已有其他修改时明确拒绝。暂停期间 undo/redo 单独记事件，分组包含暂停期间归属于该回合的用户编辑。
- [x] 批事务先校验全部命令种类/权限；只允许同文档 query/mutate，排除 file.place/save/open、history 等。临时候选逐步应用，末尾一次提交；中间失败全部丢弃。
- [x] 批内引用使用明确的结果引用类型，解析已完成步骤的稳定 ID；禁止前向/循环/跨事务引用。
- [x] 图层拖动预览单独保存临时状态，取消不改真值；松手通过 `layer.set_position` 提交一次，AI 也可直接调用。新 revision、文档关闭或控制权转移应取消旧会话。

测试目标：`undo_redo_preserves_pixels`、`undo_to_saved_state_is_clean`、`new_edit_drops_redo`、`batch_failure_rolls_back`、`external_batch_rejected_before_first_step`、`drag_cancel_is_noop`、`drag_commits_one_entry`。

映射 SYS-01/06、EDIT-01、TR-01；完整跨回合 UI 在 M2 接入。

## Task 5：分块显示与高精度合成参考

新增按可见块查询的 render snapshot 接口，替换 renderer 的单张整图 `PreparedImage`；保留 CAMetalLayer 生命周期与失败保护。

- [x] 原文档/工作 ICC → 线性预乘显示缓存；实现 Normal/Multiply/Screen/Overlay，明确计算空间、alpha 和四舍五入，独立 f64 CPU 参考作为黄金图来源。
- [x] 可见区域按块合成，纹理缓存有容量上限；未改变块不重算。修改位置/顺序/属性正确使受影响区域失效。
- [x] 缩略层不回写文档；透明边缘先预乘后过滤；块边界 halo/gutter 和 mip 跨块接缝测试。
- [x] 无编辑保存直接读源块；编辑导出走足够精度的 CPU/独立合成路径，不能从 8 位显示表面读回。GPU 与参考比较容差按输出位深定义。
- [x] 20 个独立 24MP 图层的基准工具开始记录 8/16 位；M1b 必须达到完整内存/性能门槛，当前 Mac 与标准机结果分开。

测试目标：`four_blends_match_reference`、`dirty_tile_invalidation`、`negative_position_not_clipped`、`mip_tile_edges_have_no_seam`、`display_cache_never_changes_source`。映射 SYS-07/08、LAY-06；新增平台无关合成代码须纳入可移植检查。

## Task 6：工程格式与失败安全写入

在 io 新增 `project/{format,reader,writer}.rs`、`atomic_write.rs`；序列化 DTO 放低层，避免 core→io→core 环。单文件 SQLite 为候选；先用小型往返/崩溃实验固定格式，再承诺扩展名和迁移路径。

- [x] version 0 包含画板、图层顺序/属性/稳定 ID、ICC、原位深分块；16 位样本固定字节序。块记录长度/格式/摘要验证，坏引用、未知版本、恶意尺寸或过量元数据在分配前拒绝。
- [x] 第一份工程使用同目录临时文件，完整事务、验证、同步后再原子发布；已有工程以事务保存。优先使用无常驻 WAL 边车的设置并实测事务持久性；SQLite 不是自动等于断电安全。
- [x] file.save/save_as/export 使用宿主授予的目标能力，存在目标时确认覆盖；取消在不可逆提交前生效。提交后只能报告成功，不能假装回滚。错误码明确区分发布前失败与发布后的持久性不确定。
- [x] 保存固定内容快照，记录实际保存的状态标识；保存过程中发生新编辑时当前文档仍脏。外部执行记录不进撤销栈。
- [x] PNG/TIFF 8/16 位导出包含 ICC，原生 16 位测试所有样本往返；仅在用户明确选择支持的量化方案时降低位深。
- [x] 故障注入短写、空间不足、权限失败、临时校验失败、同步失败、发布前/后强制结束；旧版或新版至少一份完整，失败不清脏标记。

测试目标：`project_roundtrip_rgba16`、`project_preserves_layer_ids_and_icc`、`unknown_version_keeps_document`、`save_during_edit_remains_dirty`、`save_fault_keeps_previous_file`、`export_png_tiff_preserves_depth`。

映射 SYS-09、FILE-04/05、IMG-07；增量恢复副本与恢复入口在 M1b，不能把工程保存误当自动恢复已完成。

## Task 7：统一门面、AppKit 与无界面工具

- [x] FFI 提供 dispatch/query snapshot/event；原生 attach/resize/render/detach 仍是受信宿主接口，不注册为 AI 文档命令。
- [x] 移除或收窄 M0 引导修改入口，AppKit 文件对话框产生授权句柄后派发文件命令；打开失败保留旧画面/历史。
- [x] 简版图层/历史面板、活动图层、脏状态、关闭确认、保存/导出对话框；用户能完成与开发工具相同的操作。
- [x] 首次创建 `tools/strata-devtool`：用隔离目录/资源句柄无界面执行同一 JSON 命令序列，导出目录、说明和逐命令示例结果。示例不能读任意用户路径或覆盖既有文件。
- [x] 清晰区分撤销项与外部保存/导出记录；失败提示用可理解的原因，不把 Rust/Swift 枚举直接展示给用户。

集成测试：UI 与 devtool 执行同一流程后，文档结构及导出像素一致；PNG/TIFF 8/16 位分别覆盖。映射 SYS-01/02、EDIT-01、FILE-01/02/04/05 及图层基础子集。

## Task 8：检查点验收与交接 M1b

- [x] `make lint`、Rust、XCTest、强制本机 Metal、Release 构建与远程 Linux/macOS CI 通过。
- [x] CI 检查所有已注册命令说明和隔离示例；映射矩阵只勾选已完成子项，不将整行多阶段功能提前标完。
- [x] 一次可重现演示：PNG/TIFF → 像素图层操作 → undo/redo → 工程保存重开 → 同位深导出；结构/ICC/样本自动比较。
- [x] 记录内存/渲染基准与文件故障注入证据，列出 M1b 剩余项；没有磁盘后备前超限应报错，不能无限分配。
- [x] M0 缺设备/人工确认项继续挂在[实机验收表](../research/2026-11-m0-hardware-checklist.md)，公开预览前补齐。私有仓库不会自动公开。

交接下一阶段：M1b 的完整文件参数/格式、最近文件、恢复入口、全局预算与磁盘后备，以及完整大图压力验收。M1a 检查点已关闭；执行证据与限制见[实施记录](../research/2026-11-m1a-progress.md)。

实现说明：模块按实际职责组织在 `commands/executor.rs`、`project.rs` 等文件中，不强制沿用计划中的占位文件名。当前多层混合使用可移植 CPU f64 分块合成，GPU 负责线性 P3 纹理、缩略采样和显示；没有宣称完整 GPU 图层混合已实现。短写/无权限/空间不足使用保存边界故障注入，另有真实进程强制退出与临时文件损坏测试；它们不等于真实硬盘断电或 SQLite VFS 逐字节短写验证。
