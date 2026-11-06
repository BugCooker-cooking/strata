# AI 编辑入口覆盖跟踪

当前开发期审计快照。完整编辑功能与最终 AI 审计仍未闭环；不得把注册数量当作全部功能覆盖。

真实注册命令 120；参数示例 174；综合功能 265；已有映射 83。

已实现但未映射 0；部分入口 9；原生未知命令引用 0；未类型化返回 0。

配置/密钥及基础设施的宿主管理理由单列；纯 UI 状态不能隐瞒为已具备独立 AI 设置命令。

| ID | 功能 | 实现 | AI 入口 | 命令 / 提示示例 / 缺口 |
|---|---|---|---|---|
| PS-00-9768e5bc84 | 命令总线 + 功能注册表 | 已实现 | 已映射 · infrastructure | `document.batch`、`browse_catalog`、`search_catalog`、`describe_commands`、`invoke_command` · 将普通图层属性修改作为一个原子批次提交；失败全部撤销，不在批次中访问文件或剪贴板。 · 通过元工具访问真实注册表，不是独立像素编辑。 |
| PS-00-4282fbce73 | 每个命令自带说明书 | 已实现 | 已映射 · infrastructure | `describe_commands` · 返回实际 schema、权限和可执行示例。 |
| PS-00-6a15d3790f | AI 接口层：浏览分类 / 读取说明 / 调用 / 查看画布与图层 | 已实现 | 已映射 · infrastructure | `document.inspect`、`document.histogram`、`layer.list`、`browse_catalog`、`search_catalog`、`describe_commands`、`inspect_document`、`view_canvas`、`invoke_command`、`invoke_batch`、`ask_user` · 查看当前授权文档的概要。 · 受宿主上下文和外发授权约束。 |
| PS-00-a9af7235f5 | AI 聊天面板 + 模型接入 | 已实现 | 已映射 · host_managed |  · 模型连接和密钥由可信宿主设置，不向 AI 暴露密钥读写。 |
| PS-00-e5b005f04b | 操作权锁（AI 执行中 / 暂停 / 终止） | 已实现 | 已映射 · host_managed | `ask_user` · 执行/暂停/停止由宿主拥有；AI 可请求暂停和澄清。 |
| PS-00-aa907aaad8 | 统一历史记录（标注来源，AI 回合可整组撤销） | 已实现 | 已映射 | `edit.undo`、`edit.redo`、`edit.undo_turn` · 撤销上一轮 AI 的编辑。 |
| PS-00-7a6d3fc6af | 无限画布 | 已实现 | 已映射 | `view.pan`、`view.zoom`、`view.fit`、`view.actual_size` · 将画布向右平移 100 像素。 |
| PS-00-81f5847182 | GPU 渲染管线 + 分块存储 | 已实现 | 已映射 · infrastructure |  · 所有编辑调用共享底层，不要求模型管理缓存。 |
| PS-00-a664122198 | Strata 工程文件格式 | 已实现 | 已映射 | `file.save`、`file.save_as`、`file.open` · 将当前文档另存为 Strata 工程。 |
| PS-00-6f14bdb57f | 自动保存与崩溃恢复 | 已实现 | 已映射 · host_managed | `file.recover` · 恢复我选择的检查点。 · 自动检查点由宿主调度；恢复通过宿主文件选择授权。 |
| PS-00-7d7224e709 | 中英文界面 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-00-960e444e99 | MCP Server | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-00-4aa7af5b95 | 插件系统 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-01-51efedefef | 新建文档（尺寸、分辨率、背景、预设） | 已实现 | 已映射 | `file.new` · 新建 1920×1080 的 16 位 sRGB 文档。 |
| PS-01-9b14920078 | 打开 PNG / JPEG / WebP / TIFF / HEIC / GIF / BMP | 已实现 | 已映射 | `file.open` · 打开我选择的照片。 |
| PS-01-b6d7cbc94d | 最近打开 | 已实现 | 已映射 | `file.recent.list` · 查看本轮允许访问的最近文件。 |
| PS-01-8f62b2552b | 保存 / 另存为（Strata 格式） | 已实现 | 已映射 | `file.save`、`file.save_as` · 将当前工程另存为新文件。 |
| PS-01-894622b1ce | 快速导出 / 导出为（格式、尺寸、质量） | 部分实现 | 部分入口 | `file.export` · 导出宽度 2048 像素的 PNG。 · 导出参数入口完整；快速导出预设本身尚未完成。 |
| PS-01-e7337088bf | 置入图片为图层 | 已实现 | 已映射 | `file.place` · 把我选择的图片置入为新图层。 |
| PS-01-265b9a61ab | 恢复到上次保存 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-01-40267007aa | PSD 导入 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-01-261e177d1e | PSD 导出 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-01-ab934d783e | 打开 RAW | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-01-b49b476915 | 置入链接文件 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-01-a69a5778e5 | 图层 / 画板导出为文件 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-01-a419a1eab8 | 批处理、图像处理器 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-01-dd3fd3e0e8 | 将文件载入堆栈 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-01-41c91ded13 | 文件简介（EXIF / IPTC 元数据） | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-01-d7bfe7b505 | 打印 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-01-1b58fee46c | PDF 导入 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-01-adb05bd33d | 全景合成、合并到 HDR | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-01-c35b70cb7c | 打包、生成图像资源、变量与数据组 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-01-bdb7c564c5 | 联系表、快捷批处理、视频帧到图层、旧版存储为 Web | 重新核对 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-01-30d15e0488 | 云文档、库、协作项目 | 重新核对 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-02-47df18296a | 撤销 / 重做 / 历史记录面板 | 已实现 | 已映射 | `edit.undo`、`edit.redo`、`edit.undo_turn` · 撤销上一个编辑步骤。 · 原生历史面板观察同一编辑历史；AI 通过历史命令操作，来源/回合仍保留。 |
| PS-02-fc9f1fdaa0 | 历史快照 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-02-24d3726654 | 剪切 / 拷贝 / 合并拷贝 / 粘贴 / 原位粘贴 / 清除 | 已实现 | 已映射 | `edit.cut`、`edit.copy`、`edit.copy_merged`、`edit.paste`、`edit.paste_in_place`、`edit.clear`、`edit.clipboard.write_status` · 剪切当前选区内容到系统剪贴板，等待宿主确认写入成功后再清除。 · 资源由可信宿主授权与写入确认；AI 不猜测 token。剪切在写入完成前不清除源；撤销只恢复文档，不能撤回系统剪贴板的外部写入。 |
| PS-02-44ec922e45 | 贴入 / 外部粘贴 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-02-03ea5fd9e6 | 填充：颜色 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-02-bfa246feb1 | 填充：内容识别 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-02-adcb808036 | 描边 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-02-f62f8ca281 | 渐隐 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-02-311ee1c4c5 | 定义画笔 / 图案 / 自定形状 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-02-57907a69fc | 偏好设置 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-02-91854349e1 | 自定义快捷键 / 菜单 / 工具栏 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-02-21ab73cf70 | 功能搜索 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-02-b2fd2e2a88 | 清理缓存 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-02-bc773443d0 | 拼写检查、查找替换文本 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-03-ad24731ea8 | 图像大小（多种重采样算法） | 部分实现 | 部分入口 | `image.resize` · 将图像大小改为512乘768像素，使用面积缩小并保留独立图层和蒙版。 · Pixels and unstyled groups have real size edits and all three sampling modes, with exact AI replacement approval and one-history turn undo. Editable fill/adjustment/style/text sizing and other professional resamplers remain open; do not interpret this partial entry as complete image sizing. |
| PS-03-45c2eea14c | 画布大小 | 已实现 | 已映射 | `image.canvas_size` · 固定画布右下角，把画布扩展到给定宽高，保留所有画板外内容，不缩放图层。 · 画布边界编辑保留结构与原码；生成填充/样式和空间调色冻结参考，选区裁到新边界，扩展覆盖为零。图像缩放与删除裁剪像素属于其他命令。 |
| PS-03-5f9716e5d8 | 图像旋转 / 翻转 | 部分实现 | 部分入口 | `image.orient`、`image.rotate`、`image.straighten` · 将整幅图像顺时针旋转 90 度，同时保留可编辑图层、链接蒙版和选区。 · All current types support exact orthogonal orientations. Pixel layers and unstyled groups support general angles and numeric/two-point straightening with optional safe inscribed crop. Generated/adjustment/style/text general geometry remains open; native canvas-dragged reference lines share the same operation/context-bound command; native/AI use identical commands with exact replacement approval and one-step undo. Native pointer input uses an operation-bound owned draft; it does not introduce a separate editing implementation or change AI parameters. |
| PS-03-84941ac6e8 | 显示全部 | 已实现 | 已映射 | `image.reveal_all` · 扩展画板，显示全部可见源图层与样式边界，恢复被画板遮住的内容。 · 以可见源图层/生成填充和自身/祖先样式的保守边界扩展，不移动/缩放内容；隐藏层不参与，蒙版/剪贴/零透明度可能让边界大于最小可见 alpha 范围。 |
| PS-03-51294954aa | 裁切（去透明边） | 已实现 | 已映射 | `image.trim` · Trim all fully transparent borders while preserving original editable layers. · Visible composite alpha>0 within the current frame, independent side booleans. No display quantization; hidden RGB/layers do not count. Sources remain stored. Tight repeat is a no-op without history; fully transparent input rejects. |
| PS-03-0f4e217715 | 复制文档 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-03-691c261ce9 | 自动色调 / 自动对比度 / 自动颜色 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-03-c10dd16a58 | RGB 模式 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-03-484489e467 | 灰度模式 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-03-67a3559782 | CMYK 模式 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-03-3e1da0c6ef | Lab / 索引色 / 双色调 / 位图 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-03-e0d8c7ed11 | 多通道 | 重新核对 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-03-692d64115b | 8 位 / 16 位 | 已实现 | 已映射 | `file.new`、`image.convert_bit_depth` · 把当前整份文档转换为16位，保留图层、蒙版和色彩配置。 · Both true conversion directions, exact integer expansion/quantization, all current editable layer and style colors, unchanged u16 masks/selections/ICC and whole-document history. Native and AI use the same operation; 16-bit expansion does not recreate precision lost by earlier 8-bit quantization. Live model/hardware/full Photoshop final audit gates remain independent. |
| PS-03-f31cf0825e | 32 位（HDR） | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-03-73a8b2cf0b | 应用图像 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-03-e44976409a | 计算 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-03-6e64fa5ccc | 分析与测量 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-03-d47b3363b5 | 陷印 | 重新核对 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-04-430fc8e2db | 新建图层 / 通过拷贝 / 通过剪切 / 背景转图层 | 已实现（M3 集成中） | 已映射 | `layer.create`、`layer.via_copy`、`layer.via_cut`、`layer.background_to_layer` · 把当前选区复制为新图层。 |
| PS-04-4e8bda1fd0 | 复制 / 删除 / 重命名 / 颜色标签 | 已实现（M3 集成中） | 已映射 | `layer.duplicate`、`layer.delete`、`layer.rename`、`layer.set_color_label` · 复制当前图层。 |
| PS-04-fe6945c814 | 图层组（可嵌套）/ 取消编组 | 已实现（M3 集成中） | 已映射 | `layer.create_group`、`layer.group`、`layer.ungroup`、`layer.reparent`、`layer.set_pass_through` · 将所选图层编组。 |
| PS-04-54bcfbb2ac | 显示隐藏 / 锁定（透明像素、像素、位置、全部） | 已实现（M3 集成中） | 已映射 | `layer.set_visibility`、`layer.set_locks` · 隐藏当前图层。 |
| PS-04-a7fe5679e6 | 不透明度 / 填充不透明度 | 已实现（M3 集成中） | 已映射 | `layer.set_opacity`、`layer.set_fill_opacity` · 把当前图层不透明度设为 50%。 |
| PS-04-bd9b0e9d03 | 混合模式（27 种） | 已实现（M3 集成中） | 已映射 | `layer.set_blend_mode` · 把当前图层混合模式设为正片叠底。 |
| PS-04-e5e334c943 | 排列顺序 / 对齐 / 分布 | 已实现（M3 集成中） | 已映射 | `layer.move_order`、`layer.align`、`layer.distribute` · 将所选图层水平居中对齐。 |
| PS-04-e77c27785e | 向下合并 / 合并可见 / 拼合图像 / 盖印 | 已实现（M3 集成中） | 已映射 | `layer.merge_down`、`layer.merge_visible`、`layer.flatten`、`layer.stamp_visible` · 把可见合成盖印到新图层。 |
| PS-04-d5e9c4039f | 图层蒙版（添加、编辑、停用、应用、反相） | 已实现 | 已映射 | `layer.mask.add`、`paint.stroke`、`paint.erase`、`layer.mask.set_enabled`、`layer.mask.apply`、`layer.mask.invert`、`layer.mask.remove` · 从当前选区添加图层蒙版。 |
| PS-04-9101e755a1 | 剪贴蒙版 | 已实现（M3 集成中） | 已映射 | `layer.set_clipping` · 把当前图层设为剪贴图层。 |
| PS-04-cb66199f15 | 调整图层 | 部分实现 | 部分入口 | `layer.add_adjustment`、`layer.add_hue_saturation`、`layer.add_tonal_adjustment`、`adjust.exposure.set`、`adjust.hue_saturation.set`、`adjust.tonal.set` · 添加可编辑的色阶调整图层。 · 当前调整类型已接入，完整后续类型待实现。 |
| PS-04-755f2b287b | 填充图层：纯色 / 渐变 | 已实现（M3 集成中） | 已映射 | `layer.fill.solid`、`layer.fill.gradient`、`layer.fill.solid.set`、`layer.fill.gradient.set` · 新建蓝色纯色填充图层。 |
| PS-04-af7f602330 | 填充图层：图案 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-04-853e6d30f3 | 栅格化 | 部分实现 | 部分入口 | `layer.rasterize` · 栅格化当前填充图层。 · 当前支持类型已接入，文字/智能对象类型待后续实现。 |
| PS-04-d6c3930e2f | 图层样式：投影、描边、外发光、颜色叠加、渐变叠加 | 已实现（M3 集成中） | 已映射 | `layer.set_styles` · 给当前图层添加投影和描边。 |
| PS-04-6084cae48d | 图层样式：内阴影、内发光、斜面浮雕、光泽、图案叠加；样式预设 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-04-e65a84f08f | 链接图层 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-04-288c65e42f | 混合颜色带（Blend If） | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-04-ef23225e49 | 挖空 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-04-c9a10eae37 | 智能对象：转换、编辑内容、替换内容、栅格化 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-04-9b3a04c666 | 智能滤镜 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-04-9383ee3abd | 矢量蒙版 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-04-dd95b3b697 | 图层筛选与查找 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-04-53f23afed6 | 画板 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-04-f22dbf0e07 | 图层复合 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-04-fc1ecbb499 | 修边（去边、移除黑 / 白杂边） | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-04-d31acdcaa9 | 视频图层 | 重新核对 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-05-24f905f241 | 矩形 / 椭圆选框 | 已实现 | 已映射 | `select.marquee`、`select.ellipse` · 建立一个椭圆选区。 |
| PS-05-8885dd3689 | 套索 / 多边形套索 | 已实现 | 已映射 | `select.lasso`、`select.polygon` · 用多边形圈选给定区域。 |
| PS-05-d2a511da9e | 魔棒 | 已实现 | 已映射 | `select.wand` · 用容差 24 的魔棒选择给定位置相近颜色。 |
| PS-05-825f270266 | 快速选择 | 已实现 | 已映射 | `select.quick` · 根据前景与背景笔触进行快速选择。 |
| PS-05-f88b2ecb2f | 选择主体 🤖 | 已实现 | 已映射 | `select.subject` · 选择照片主体。 |
| PS-05-c3053c1de1 | 全选 / 取消选择 / 重新选择 / 反选 | 已实现 | 已映射 | `select.all`、`select.none`、`select.reselect`、`select.inverse` · 反选当前选区。 |
| PS-05-a8345e1798 | 选区相加 / 相减 / 交叉 | 已实现 | 已映射 | `select.marquee`、`select.ellipse`、`select.polygon`、`select.lasso` · 把新矩形区域加入当前选区。 |
| PS-05-2cd4b1dc43 | 羽化 / 扩展 / 收缩 / 平滑 / 边界 | 已实现 | 已映射 | `select.feather`、`select.expand`、`select.contract`、`select.smooth`、`select.border` · 把当前选区羽化 5 像素。 |
| PS-05-1f294f54d0 | 变换选区 | 已实现 | 已映射 | `select.transform` · 旋转并放大当前选区。 |
| PS-05-9bfbc83e75 | 磁性套索 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-05-aa44dc16d9 | 对象选择 🤖 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-05-9ac4e17211 | 选择天空 🤖 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-05-38d4ee4918 | 按文字描述选择（如"选中左边的人"）🤖 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-05-8d060c56cd | 扩大选取 / 选取相似 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-05-2998b53723 | 色彩范围 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-05-4ed8bb8e25 | 快速蒙版 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-05-60905d6faf | 存储 / 载入选区（Alpha 通道） | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-05-f7f00647a1 | 选择并遮住（细化边缘，如头发） | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-05-6a8c7914d1 | 单行 / 单列选框 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-05-96d3b2c5d9 | 焦点区域 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-06-d51749f4f4 | 亮度 / 对比度 | 已实现 | 已映射 | `layer.add_tonal_adjustment`、`adjust.tonal.set` · 添加可编辑的亮度 / 对比度调整。 |
| PS-06-4341979e35 | 色阶 | 已实现 | 已映射 | `layer.add_tonal_adjustment`、`adjust.tonal.set` · 添加可编辑的色阶调整。 |
| PS-06-937dc7bf30 | 曲线 | 已实现 | 已映射 | `layer.add_tonal_adjustment`、`adjust.tonal.set` · 添加可编辑的曲线调整。 |
| PS-06-b941862025 | 曝光度 | 已实现 | 已映射 | `layer.add_adjustment`、`adjust.exposure.set` · 增加半档曝光。 |
| PS-06-e7e1ea6652 | 自然饱和度 + 色温 / 色调 | 已实现 | 已映射 | `layer.add_tonal_adjustment`、`adjust.tonal.set` · 添加可编辑的自然饱和度 + 色温 / 色调调整。 |
| PS-06-65bc8e9d6e | 色相 / 饱和度 | 已实现 | 已映射 | `layer.add_hue_saturation`、`adjust.hue_saturation.set` · 降低当前图层的饱和度。 |
| PS-06-cdc392db20 | 色彩平衡 | 已实现 | 已映射 | `layer.add_tonal_adjustment`、`adjust.tonal.set` · 添加可编辑的色彩平衡调整。 |
| PS-06-47d88357c5 | 黑白 | 已实现 | 已映射 | `layer.add_tonal_adjustment`、`adjust.tonal.set` · 添加可编辑的黑白调整。 |
| PS-06-2263716466 | 照片滤镜 | 已实现 | 已映射 | `layer.add_tonal_adjustment`、`adjust.tonal.set` · 添加可编辑的照片滤镜调整。 |
| PS-06-7fec4dc313 | 通道混合器 | 已实现 | 已映射 | `layer.add_tonal_adjustment`、`adjust.tonal.set` · 添加可编辑的通道混合器调整。 |
| PS-06-39bbb078d5 | 反相 / 色调分离 / 阈值 | 已实现 | 已映射 | `layer.add_tonal_adjustment`、`adjust.tonal.set` · 添加可编辑的反相 / 色调分离 / 阈值调整。 |
| PS-06-1769d6d1d6 | 渐变映射 | 已实现 | 已映射 | `layer.add_tonal_adjustment`、`adjust.tonal.set` · 添加可编辑的渐变映射调整。 |
| PS-06-63924bf039 | 可选颜色 | 已实现 | 已映射 | `layer.add_tonal_adjustment`、`adjust.tonal.set` · 添加可编辑的可选颜色调整。 |
| PS-06-ef2316cc3c | 阴影 / 高光 | 已实现 | 已映射 | `layer.add_tonal_adjustment`、`adjust.tonal.set` · 添加可编辑的阴影 / 高光调整。 |
| PS-06-be1d0be625 | 去色 | 已实现 | 已映射 | `layer.add_tonal_adjustment`、`adjust.tonal.set` · 添加可编辑的去色调整。 |
| PS-06-3032971663 | 清晰度 / 去朦胧 / 纹理 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-06-2888ee744e | 颜色查找（LUT） | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-06-7b4433741f | 匹配颜色 / 替换颜色 / 色调均化 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-06-ff679aec47 | 调整预设 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-06-77f246179b | HDR 色调 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-07-7fd32754e9 | 污点修复画笔 | 已实现 | 已映射 | `retouch.spot_heal` · 自动修复给定笔触内的小污点。 |
| PS-07-2eddb4a1cf | 修复画笔 | 已实现 | 已映射 | `retouch.heal` · 用指定源点的纹理修复目标区域并匹配边界。 |
| PS-07-ec6361e1f3 | 仿制图章 | 已实现 | 已映射 | `retouch.clone` · 从指定源点对齐仿制纹理。 |
| PS-07-74865c42fd | 减淡 / 加深 / 海绵 | 已实现 | 已映射 | `retouch.dodge`、`retouch.burn`、`retouch.sponge` · 用软笔刷轻微减淡中间调。 |
| PS-07-99d39b0e3f | 修补工具 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-07-567e47777f | 内容识别填充 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-07-9411b77a01 | 移除工具 🤖 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-07-a39f54a54c | 红眼工具 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-07-2ad3989bf6 | 模糊 / 锐化 / 涂抹工具 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-07-a2d1d27aed | 液化 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-07-dfaf73f366 | 内容识别移动 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-07-a63d0d97e2 | 仿制源面板 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-07-ba6e326669 | 图案图章 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-08-13d3081dfa | 画笔（大小、硬度、不透明度、流量、间距、压感） | 已实现 | 已映射 | `paint.stroke` · 用 24 像素软红色画笔绘制给定轨迹。 |
| PS-08-05cf56410b | 橡皮擦 | 已实现 | 已映射 | `paint.erase` · 用软橡皮擦擦除给定轨迹。 |
| PS-08-af9951e405 | 油漆桶 | 已实现 | 已映射 | `paint.bucket` · 填充给定位置的相连区域。 |
| PS-08-739094bd86 | 渐变（线性 / 径向 / 角度 / 对称 / 菱形） | 已实现 | 已映射 | `paint.gradient` · 在给定范围绘制径向渐变。 |
| PS-08-6f11ed20eb | 吸管 | 已实现 | 已映射 | `paint.sample_color` · 吸取给定位置的可见合成颜色。 |
| PS-08-3ce0368d80 | 拾色器、前景色 / 背景色 | 已实现 | 已映射 | `paint.colors.get`、`paint.colors.set`、`paint.colors.swap`、`paint.colors.reset`、`paint.sample_color`、`paint.stroke`、`paint.fill`、`paint.gradient` · 读取当前前景色和背景色及调色板版本。 · 原生色块、吸管、X/D 与 AI 走同一编辑器工具状态。工作 RGB 浮点 RGBA 与独立版本绑定；不修改文档样本/历史/dirty，也不由文档或 AI 回合撤销恢复。绘画仍显式传色，AI 可读取调色板并使用其前景/背景参数。 |
| PS-08-86b51af036 | 颜色面板 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-08-92b84304ee | 颜色取样器 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-08-8aec3995bf | 色板 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-08-f77e30ee1a | 铅笔 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-08-94f49a1012 | 画笔设置（形状动态、散布、纹理、双重画笔……） | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-08-1c4da8d02e | 画笔 / 渐变预设管理 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-08-c236d7a8cc | 图案预设管理 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-08-d00f4ae949 | ABR 画笔导入 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-08-d5cde59fb9 | 背景橡皮擦 / 魔术橡皮擦 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-08-5bc77239a4 | 颜色替换 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-08-2b199ba19e | 历史记录画笔 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-08-20a38a31c0 | 混合器画笔 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-08-8917363943 | 历史记录艺术画笔 | 重新核对 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-09-7cc6b0bb65 | 移动工具（含吸附） | 部分实现 | 部分入口 | `layer.select`、`layer.set_position` · 选择可见目标图层，使用稳定 ID。 · 已有整数位置与原生拖动、单次提交/取消及组联动；吸附仍未实现，因此综合移动工具条目保持部分实现。 |
| PS-09-4f3d9835a7 | 自由变换：缩放 / 旋转 / 斜切 / 扭曲 / 透视 / 翻转 | 部分实现 | 部分入口 | `layer.pixel_bounds`、`transform.apply` · 把整个像素图层围绕给定轴心旋转 15 度，保留画板外内容。 · 整层像素/链接蒙版及选区像素的真实提升/变换/合回已接入；软边按 source-over 余量保留，目标不被原选区重复裁切。生成/组/文字自由变换及交互手柄仍未实现。 |
| PS-09-7d349372b5 | 裁剪工具（拉直、比例、是否删除裁剪像素） | 部分实现 | 部分入口 | `image.crop`、`image.straighten` · 用指定画布矩形和 4:3 比例裁剪画板，保留所有裁剪外像素与图层。 · Numeric rectangle/aspect, owned canvas drag/handles/move and preserve/delete share real crop commands. Numeric or two-point horizontal/vertical straightening composes rotation and a safe preserving crop in one history step. Canvas-dragged lines stage a guide, bind original document/revision/control/operation, and confirm through the same image.straighten command. Generated/adjustment/style/text arbitrary geometry remains open. Native pointer input uses an operation-bound owned draft; it does not introduce a separate editing implementation or change AI parameters. |
| PS-09-64f3d2a3eb | 变形 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-09-073c032f33 | 透视裁剪 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-09-6891bcf56f | 操控变形 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-09-cfef658a75 | 透视变形 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-09-bf3489a74b | 内容识别缩放 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-09-b8404e61aa | 自动对齐图层 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-09-d278290204 | 自动混合图层 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-09-71958254c4 | 旋转对象 🤖（2026 新增） | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-0e134f5a85 | 高斯模糊 | 已实现 | 已映射 | `filter.gaussian_blur` · 把当前像素图层做标准差 2px 的高斯模糊。 |
| PS-10-5785be70cb | USM 锐化 | 已实现 | 已映射 | `filter.unsharp_mask` · 对当前像素图层做数量 120%、标准差 1px 的 USM 锐化。 |
| PS-10-395d09a3e1 | 添加杂色 | 已实现 | 已映射 | `filter.add_noise` · 给当前像素图层添加 2% 单色高斯颗粒，种子为 42。 |
| PS-10-03bd81f36a | 智能锐化 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-10e03c578e | 高反差保留 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-c323ce2dc1 | 减少杂色 / 中间值 / 蒙尘与划痕 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-4cd844c0cb | 动感模糊 / 表面模糊 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-a5da48d57a | 最大值 / 最小值 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-388913a7bc | 径向模糊 / 镜头模糊 / 方框模糊 / 平均 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-7b85bb5f1a | 模糊画廊：场景 / 光圈 / 移轴 / 路径 / 旋转 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-96038cde01 | 镜头校正 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-9ce22316bf | 位移 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-14619cadb7 | 马赛克 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-4910186cb0 | 云彩 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-9be49e6137 | 扭曲组：置换 / 挤压 / 极坐标 / 波纹 / 切变 / 球面化 / 旋转扭曲 / 波浪 / 水波 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-344b92cabe | 像素化组其余：彩色半调 / 晶格化 / 点状化 / 碎片 / 铜版雕刻 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-75ea494817 | 渲染组：镜头光晕 / 光照效果 / 纤维 / 分层云彩 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-0672d98893 | 风格化组：浮雕 / 查找边缘 / 油画 / 风 / 扩散 / 曝光过度 / 拼贴 / 凸出 / 等高线 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-d4877d09e6 | 自定 / HSB/HSL | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-7c8555eca5 | 自适应广角 / 消失点 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-c5d0281f10 | 滤镜库（47 个旧式艺术滤镜） | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-dca9e3ce4b | 火焰 / 树 / 图片框 | 重新核对 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-10-1e9f8da0f7 | 防抖 | 重新核对 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-11-4490121dad | 横排文字（点文字 / 段落文字） | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-11-a524877cb6 | 字体、字号、颜色、行距、字距、对齐 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-11-cf9b897491 | 栅格化文字 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-11-c6c65f4de6 | 直排文字 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-11-0a97b13dc8 | 文字变形 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-11-4bfa8a3abe | 路径文字 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-11-f3c09ca56f | 字符样式 / 段落样式 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-11-346810064b | OpenType 特性 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-11-07a96c386e | 文字转形状 / 创建工作路径 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-11-d0e9236cfa | 文字蒙版工具 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-11-b2feb2a72d | 字形面板 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-11-0907b4762e | 匹配字体 🤖 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-12-417500cf30 | 形状：矩形（含圆角）/ 椭圆 / 直线 / 多边形 / 三角形 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-12-a132c853ab | 钢笔 / 弯度钢笔 / 自由钢笔 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-12-595e7321ce | 添加 / 删除 / 转换锚点；路径选择 / 直接选择 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-12-cfd8ae8285 | 路径面板、路径与选区互转 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-12-2e23400f5e | 形状组合（合并 / 减去 / 相交） | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-12-9bae2bd626 | 自定形状 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-12-afd1a689fb | SVG 导入导出 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-13-bbaac8cf76 | RAW 解码（主流相机） | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-13-f7bdbf0b8e | 基本面板：白平衡、曝光、对比、高光、阴影、白色、黑色、纹理、清晰度、去朦胧 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-13-fc3a065bd4 | 曲线 / 混色器（HSL）/ 颜色分级 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-13-31102e180c | 细节：锐化、降噪 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-13-41b283aec3 | 效果：颗粒、晕影 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-13-f1de1e2621 | 预设 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-13-0529cd2d51 | 光学：镜头配置文件、色差、暗角 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-13-4592c9e11f | 几何：Upright 透视校正 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-13-1b8faab964 | AI 蒙版（主体 / 天空 / 人物）🤖、AI 降噪 🤖、镜头模糊 🤖 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-14-cf2715aeb6 | 缩放 / 抓手 / 适合屏幕 / 100% | 已实现 | 已映射 | `view.pan`、`view.zoom`、`view.fit`、`view.actual_size` · 以实际像素 100% 显示当前画布，保持中心不变。 · 绝对 100% 与相对缩放分开；视图命令不改变文档或历史。 |
| PS-14-c9a6761937 | 多文档标签页 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-14-de2fd3ca59 | 标尺 / 参考线 / 对齐吸附 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-14-810f37be85 | 旋转视图 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-14-15d282cc9a | 智能参考线 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-14-3e68cd79ce | 网格 / 像素网格 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-14-8ae0da2a42 | 导航器 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-14-09e4a9ec7d | 多窗口排列 / 屏幕模式 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-14-918e81018b | 校样颜色 / 色域警告 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-14-8430a881e8 | 新建参考线版面 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-15-d3094c6954 | 查看单个颜色通道 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-15-b99621b074 | Alpha 通道（存储选区） | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-15-c4509c8344 | 复制通道、通道转选区 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-15-117606acae | 专色通道 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-16-468033d33b | 工作色彩空间 sRGB / Display P3 | 已实现 | 已映射 | `file.new`、`color.convert`、`color.assign` · 新建 Display P3 工作空间的文档。 |
| PS-16-4f3db94f33 | 读取并应用图片内嵌的 ICC 配置文件 | 已实现 | 已映射 | `file.open` · 打开我选择的带 ICC 配置文件照片。 |
| PS-16-b59d8280a5 | 导出时嵌入 / 转换配置文件 | 已实现 | 已映射 | `file.export` · 导出时转换并嵌入 Display P3 配置文件。 |
| PS-16-8de06204ac | 指定 / 转换配置文件 | 部分实现 | 部分入口 | `color.assign`、`color.convert` · 指定 Display P3 并保留 RGB 数值，不转换像素。 · 当前指定支持两种内置 RGB 预设；任意 ICC 文件与 CMYK 设置待后续。 |
| PS-16-56a0bc682a | 软打样 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-16-e269e999cb | 颜色设置（自定义 CMYK 等） | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-17-86de52d178 | 属性 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-17-b487bf0eba | 信息、直方图 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-17-01b402a5c6 | 动作、样式、图层复合、上下文任务栏 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-17-48bd1a145f | 工具预设、测量记录、注释 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-17-8368e3ab8e | 时间轴、库、评论、版本历史 | 重新核对 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-18-5d4f709636 | 脚本 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-18-1d67f7829a | 动作：录制 / 回放 / 编辑 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-18-c9f7c41baa | 批处理 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-18-4f30ead3eb | 插件（类似 Photoshop 的 UXP） | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-19-21fdf95781 | 移除背景 🤖 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-19-068a4aa8c2 | 生成式填充 🤖 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-19-90afa613b4 | 生成式扩展 🤖 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-19-320004af59 | 超分辨率 / 生成式放大 🤖 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-19-0af30ae366 | 天空替换 🤖 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-19-4f4a33e4db | 生成图像 / 生成背景 / 生成相似 🤖 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-19-6e42c7e07e | 协调（合成时自动匹配光照与色彩）🤖 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-19-d9de9483c3 | 移除干扰物（行人、电线）/ 反射去除 🤖 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |
| PS-19-ef0365d914 | Neural Filters：皮肤平滑、智能肖像、上色、超级缩放、JPEG 伪影去除、照片修复、风格转换、妆容迁移、景观混合器、深度模糊、色彩转移 🤖 | 待实现 | 待实现/待核对 | 保留最终入口与提示核对任务 |

## 仍需核对

- 部分入口：PS-01-894622b1ce、PS-04-cb66199f15、PS-04-853e6d30f3、PS-16-8de06204ac、PS-09-4f3d9835a7、PS-09-7cc6b0bb65、PS-09-7d349372b5、PS-03-5f9716e5d8、PS-03-ad24731ea8
- 未类型化返回：无
- 未关联功能条目的命令：
- 动态拼接及纯宿主回调仍需人工逐项复核；已有提示映射不能代替最终模型端到端验收。
- `--require-complete` 才是最终严格门槛；开发期检查只保证引用有效及报告同步。
