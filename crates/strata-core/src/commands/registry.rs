use super::*;
use serde_json::json;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    Query,
    Mutate,
    View,
    External,
    Lifecycle,
    History,
}

#[derive(Serialize)]
pub struct CommandSpec {
    pub id: &'static str,
    pub path: Vec<&'static str>,
    pub title_zh: &'static str,
    pub title_en: &'static str,
    pub summary: &'static str,
    pub description: &'static str,
    pub kind: CommandKind,
    pub effects: Vec<&'static str>,
    pub permissions: Vec<&'static str>,
    pub batchable: bool,
    pub undoable: bool,
    pub params: schemars::Schema,
    pub returns: schemars::Schema,
    pub preconditions: Vec<&'static str>,
    pub examples: Vec<Value>,
}

pub fn catalog() -> &'static [CommandSpec] {
    static CATALOG: std::sync::LazyLock<Vec<CommandSpec>> = std::sync::LazyLock::new(build_catalog);
    &CATALOG
}
fn build_catalog() -> Vec<CommandSpec> {
    let mut list = Vec::new();
    macro_rules! command {
        ($id:expr,$params:ty,$kind:ident,$zh:expr,$en:expr,$description:expr,$example:expr) => {{
            let kind=CommandKind::$kind;
            list.push(CommandSpec { id:$id,path:($id).split('.').collect(),title_zh:$zh,title_en:$en,summary:$en,description:$description,kind,
                effects:match kind { CommandKind::Query=>vec!["document.read"], CommandKind::View=>vec!["view.write"],_=>vec!["document.write"] },
                permissions:vec!["bound_document"],batchable:matches!(kind,CommandKind::Query|CommandKind::Mutate),undoable:kind==CommandKind::Mutate,
                params:schemars::schema_for!($params),returns:schemars::schema_for!(Value),
                preconditions:vec!["Host-bound source, session, document and revision; control ownership and resource limits are checked."],examples:vec![$example] });
        }}
    }
    command!(
        "view.guides.list",
        ListGuides,
        Query,
        "读取参考线",
        "List canvas guides",
        "Read a bounded page of document reference lines. offset0..1024(default0), limit1..64(default32), total and next_offset support pagination. Each line has a stable id, name up to256 UTF8 bytes without controls, horizontal(y) or vertical(x) axis and finite absolute canvas position +/-1e12. Prefer ids; duplicate names are never guessed. Guide visibility/locking is editor presentation state, not privacy or source deletion. A document is required; no pixel/history/revision/selection effect. Not batchable. 中文：标尺参考线、读取参考线、水平线、垂直线、参考线坐标、分页。",
        json!({"limit":32})
    );
    command!(
        "view.guides.add",
        AddGuide,
        Mutate,
        "新建参考线",
        "Add canvas guide",
        "Add one actual horizontal(y) or vertical(x) document guide, with name up to256 UTF8 bytes without control characters and finite position +/-1e12 in absolute canvas coordinates. At most1024 lines; server creates the stable id. Guides are nonexported references: crop/resize/rotate content leaves these canvas-axis coordinates fixed. They persist in manifest9 and undo/recovery; no source pixel, mask, ICC or layer edit. Locked guides reject. Creation intentionally adds a new line, including coincident lines; same call-id retry is idempotent. One history, not batchable. Native ruler placement and AI use the same command. 中文：新建参考线、水平参考线、垂直参考线、从标尺拖出参考线、画布绝对坐标；不烘入图片，可一次撤销。",
        json!({"name":"Baseline","axis":"horizontal","position":6.5})
    );
    command!(
        "view.guides.update",
        UpdateGuide,
        Mutate,
        "修改参考线",
        "Update canvas guide",
        "Update an explicit guide's axis, position and/or name, preserving its stable id. Read view.guides.list first and prefer guide.id; ambiguous names return candidate ids. Same coordinate/name/axis returns changed=false without document history/revision/dirty/selection changes. Locked guides reject; other add limits and persistence semantics apply. One atomic edit, not batchable; native numeric and pointer edits use this command. 中文：修改参考线、移动参考线、参考线重命名、参考线方向；相同值无历史。",
        json!({"guide":{"name":"Baseline"},"position":10.5})
    );
    command!(
        "view.guides.delete",
        TargetGuide,
        Mutate,
        "删除参考线",
        "Delete canvas guide",
        "Delete exactly one explicit document guide and return that removed line. Prefer its id from the paginated list; ambiguous names reject. Pixels and layers are unchanged, one undo restores the exact id/name/axis/position. Locked guides reject; not batchable. 中文：删除参考线、移除参考线；只移除指定参考线，可撤销。",
        json!({"guide":{"name":"Baseline"}})
    );
    command!(
        "view.guides.clear",
        Empty,
        Mutate,
        "清除参考线",
        "Clear canvas guides",
        "Remove every reference line in the bound document; return removed count. Locked guides reject. Empty documents return changed=false without history/revision/dirty/selection changes. A real removal is one history and exact undo; source samples/layers stay intact. Not batchable. 中文：清除参考线、删除全部参考线；仅当前文档，一次撤销。",
        json!({})
    );
    command!(
        "view.presentation.get",
        Empty,
        Query,
        "读取标尺与吸附设置",
        "Inspect ruler and snap presentation",
        "Read editor-local rulers_visible/guides_visible/guides_locked/snap_enabled/snap_tolerance/snap_artboard and version. snap_tolerance is native screen points0..64(default6); explicit layer.snap_position uses canvas units. Defaults show rulers/guides, unlock guides and disable automatic snapping. State is independent of document history/save/undo and can be read without a document. Native and AI share it. 中文：读取视图设置、标尺显示、参考线显示、锁定参考线、吸附设置、吸附阈值。",
        json!({})
    );
    command!(
        "view.presentation.set",
        SetPresentation,
        View,
        "设置标尺与吸附",
        "Set ruler and snap presentation",
        "Set one or more editor-local presentation fields; expected_version optionally binds a preceding get. Equal values do not advance the version; real changes advance it once without document history/revision/dirty/current-layer changes. Guides themselves are document data edited through view.guides.*. Guide locks prohibit line edits but not snapping to those references. Automatic V-tool snapping uses the current zoom to convert native screen-point tolerance0..64 to canvas units; snap_artboard includes artboard edges. An old pointer draft cannot commit after presentation version changes. Viewing may change during AI control, while guide content edits still require normal document ownership. No document needed, not batchable. 中文：显示标尺、隐藏标尺、显示参考线、隐藏参考线、锁定参考线、开启吸附、关闭吸附、画板边缘吸附；只改变当前编辑器视图，不写历史。",
        json!({"rulers_visible":true,"guides_visible":true,"snap_enabled":true,"snap_tolerance":6})
    );
    command!(
        "layer.snap_position",
        SnapPosition,
        Mutate,
        "吸附移动图层",
        "Move layer with guide snapping",
        "Move an explicit visible pixel/text/fill layer or visible group subtree to proposed integer x/y +/-1e12, then resolve nearest reference-line/artboard-edge alignment independently per axis. guides/artboard defaulttrue, anchors edges/center/edges_and_center(default), tolerance finite canvas pixels0..1e6(default6). Only visible subtree content contributes bounds; hidden descendants still move with the group without exposing their bounds. Include off-artboard source and alpha-zero hidden RGB; empty text caches use retained glyph ink. Adjustments alone have no content box. Sparse scans capped64M stored visits and64M measured child areas, pooled/cancellable text work. Ties prefer guide over artboard, lower reference coordinate, edge-before-center order and stable guide id. Proposals snap to the nearest integer layer position, rounding ties away from zero; returned anchor/residual explicitly report fractional guide mismatch rather than claiming subpixel movement. Preserve source codes/tile versions, layer kind/text source, linked masks, tree and ICC/depth. Reject target/subtree/ancestor position/all locks, without implicit rasterization. changed=false preserves history/revision/dirty/current-layer; otherwise one atomic history and undo. Native V-tool and numeric UI share the same resolver; immutable gesture bounds are trusted host context, never model parameters. Not batchable. 中文：移动吸附、参考线吸附、边缘对齐、中心对齐、画板边缘、吸附阈值；移动按整数像素，返回半像素残差，原码与蒙版保留。",
        json!({"layer":{"name":"Background"},"x":3,"y":3,"tolerance":4,"artboard":false})
    );
    command!(
        "type.fonts",
        Empty,
        Query,
        "读取内置字体",
        "List bundled fonts",
        "List the three fixed, licensed bundled font families sans(Noto Sans), serif(Noto Serif) and cjk(Noto Sans CJK SC), their font hashes and typography version. No host system paths/fonts/credentials are exposed or scanned. Latin families use the same fixed CJK fallback; files/OS font import and additional scripts remain separate future work. Query does not change any document/tool/history/current-layer state and can run without a document. 中文：字体列表、内置字体、字族、Noto Sans、Noto Serif、中文字体、可用字体。",
        json!({})
    );
    command!(
        "type.create",
        CreateText,
        Mutate,
        "新建可编辑文字",
        "Create editable text",
        "Create a real editable horizontal point/paragraph text layer, retaining Unicode source and complete uniform style. name and spec required; spec.text <=65536 UTF8 bytes/16384 characters, font=sans(default)/serif/cjk, font_size1..2048 canvas px(default32), line_height.5..10 em(default1.2), tracking -256..2048px(default0, also at least -font_size/2), align left/center/right/justified(defaultleft), layout.type=point or paragraph with integer width/height1..100000. Point x/y is the first-line baseline anchor (center/right alignment offsets each line from x); paragraph x/y is its top-left and ink clips to its width/height while full overflow source is retained. Fixed-version bundled OFL fonts, advanced portable Rust shaping/bidi/word-or-glyph CJK wrapping and grayscale glyph coverage; no system font lookup, no display screenshots. Missing glyphs reject rather than silently replacing with tofu; color emoji, vertical/path/rich-span type and arbitrary font import remain unsupported. color is normalized straight encoded document working RGBA0..1, quantized once to document8/16-bit codes; glyph alpha coverage is8-bit expanded to u16 with exact integer source-over for overlaps. Current selection does not cut text. Layer/group masks, blend/clipping/fill-opacity and editable styles use the same compositor as pixel layers. Font parse/shaping/glyph visits/ink bounds/tiles reserve shared memory,32M pixel/glyph visits, row/glyph cancellation; one atomic history and exact undo/project/recovery. x/y integer canvas positions within +/-1e12. Native text editing and AI share this implementation. 中文：文字、文字工具、点文字、段落文字、创建中文文字层、可编辑文本、字体字号、行距、字距、左中右对齐、中文换行；原文和样式保留，图层可单步撤销。",
        json!({"name":"Title","spec":{"text":"Strata 中文","font":"sans","font_size":24,"layout":{"type":"point"}},"color":[0.2,0.4,0.8,1],"x":2,"y":30})
    );
    command!(
        "type.update",
        UpdateText,
        Mutate,
        "修改可编辑文字",
        "Update editable text",
        "Replace the complete text spec and normalized working RGBA color of an explicit editable text layer through the same portable shaper/raster/compositor as type.create. Use type.inspect first, preserve its original text/style fields you are not changing, and supply full spec; input never takes a system font path or image snapshot. Preserve layer ID/name/position/visibility/opacity/blend/clipping/parent/styles and u16 mask/selection. Retained geometry/crop/size operations replay from the new source; uniform RGB stays exact and only glyph coverage is sampled. Adjacent transforms compose before sampling, crop barriers retain operation order; at most32 retained stages and64M transform destination visits per text re-render. Text metrics describe original source layout; layer.pixel_bounds reports final transformed ink. If a longer source crosses a stored projective horizon, reject atomically and keep old source. Destructive-crop local support remains permanent for later edits, preventing discarded text ink from regrowing; source text stays editable. Pixel/transparency/all or ancestor locks reject; position-only locks allow editing. Identical validated spec and final color codes return changed=false without history/revision/dirty/current-layer change; otherwise publish once with exact undo. Same text length/font/glyph/memory/cancellation limits as creation. Rendering cache is derived from retained source/style at document depth, not an editable screenshot. Native and AI use the same command. 中文：编辑文字、修改文本、替换文字内容、字体、字号、文字颜色、行距、字距、对齐、段落宽高、文字属性；参数完整替换，一次撤销。",
        json!({"layer":{"name":"Title"},"spec":{"text":"Edited 中文","font":"serif","font_size":20,"line_height":1.4,"tracking":0.5,"align":"center","layout":{"type":"paragraph","width":140,"height":70}},"color":[0.8,0.2,0.1,0.7]})
    );
    command!(
        "type.inspect",
        Target,
        Query,
        "读取文字源和排版",
        "Inspect editable text",
        "Read source Unicode, full uniform type spec, pinned typography version and encoded8/16-bit working RGBA codes from an explicit visible editable text layer with visible ancestors; metrics is null on this lightweight query. definition.operations (omitted if empty) retains local-coordinate transforms/crop barriers/resizes in execution order. type.update preserves them without accepting geometry as text parameters. Use transform.apply or image geometry commands to change geometry. Use the document bit depth to normalize definition.color codes before sending type.update (divide by255 or65535). It never shapes, rasterizes, grants font/file resources or changes selection/history/revision/current layer. Missing/nontext/hidden targets reject. 中文：读取文字、文字内容、文字排版、字体属性、可编辑文本源、段落参数。",
        json!({"layer":{"name":"Title"}})
    );
    command!(
        "file.new",
        NewDocument,
        Lifecycle,
        "新建文档",
        "New document",
        "Create one artboard and a pixel background. Supply positive width/height or preset web_1080p, a4_300dpi, photo_4x6_300dpi, square_2048 (explicit dimensions must match). bit_depth is 8/16; resolution_dpi is integer 1..2400 (preset default, otherwise 72); working_space is srgb/display_p3; background is optional RGBA at document depth, otherwise sparse transparent. Preset metadata is kept only for exact geometry/density. Replacing dirty content requires discard_changes.",
        json!({"width":16,"height":16,"bit_depth":16})
    );
    command!(
        "document.inspect",
        Empty,
        Query,
        "文档信息",
        "Inspect document",
        "Read metadata, revision and saved state for the bound document.",
        json!({})
    );
    command!(
        "file.recent.list",
        Empty,
        Query,
        "最近文件",
        "List recent files",
        "Read host-provided recent metadata (maximum 10). User/Test sees host history; AI sees only entries with a still-valid read resource for that exact turn/source in this editor session, with paths omitted. Listing never grants resources. Availability is advisory. Allowed with no document; not batchable.",
        json!({})
    );
    command!(
        "layer.list",
        Empty,
        Query,
        "图层列表",
        "List layers",
        "Read layers in bottom-to-top order. Stable IDs are preferred over names.",
        json!({})
    );
    command!(
        "document.histogram",
        Empty,
        Query,
        "亮度直方图",
        "Luminance histogram",
        "Read a bounded 256-bin linear luminance histogram of the visible composite, sampled at up to 256 by 256 pixel centers. It is a display statistic, not document samples.",
        json!({})
    );
    command!(
        "layer.create",
        CreateLayer,
        Mutate,
        "新建图层",
        "Create layer",
        "Create an artboard-sized pixel layer. Optional fill is RGBA integers at document depth; otherwise sparse transparent.",
        json!({"name":"New layer"})
    );
    command!(
        "layer.delete",
        Target,
        Mutate,
        "删除图层",
        "Delete layer",
        "Delete a pixel layer. Undo restores its stable ID and samples.",
        json!({"layer":{"name":"Background"}})
    );
    command!(
        "layer.duplicate",
        Target,
        Mutate,
        "复制图层",
        "Duplicate layer",
        "Duplicate a layer with a fresh ID and shared copy-on-write pixels.",
        json!({"layer":{"name":"Background"}})
    );
    command!(
        "layer.rename",
        Rename,
        Mutate,
        "重命名图层",
        "Rename layer",
        "Rename a layer. Duplicate names are permitted but cannot be resolved ambiguously.",
        json!({"layer":{"name":"Background"},"name":"Photo"})
    );
    command!(
        "layer.set_visibility",
        Visibility,
        Mutate,
        "图层显隐",
        "Set visibility",
        "Change persistent layer visibility. This edit is allowed for the user during AI pause.",
        json!({"layer":{"name":"Background"},"visible":false})
    );
    command!(
        "layer.set_opacity",
        Opacity,
        Mutate,
        "图层不透明度",
        "Set opacity",
        "Set finite opacity in [0,1]; invalid values leave the document unchanged.",
        json!({"layer":{"name":"Background"},"opacity":0.5})
    );
    command!(
        "layer.set_blend_mode",
        Blend,
        Mutate,
        "混合模式",
        "Set blend mode",
        "Choose one of 27 registered blend modes. Blending operates on linear Display P3; source document samples remain unchanged. Dissolve uses deterministic canvas-coordinate and layer-ID noise, so repeats never flicker.",
        json!({"layer":{"name":"Background"},"blend_mode":"multiply"})
    );
    command!(
        "layer.set_position",
        Position,
        Mutate,
        "移动图层",
        "Move layer",
        "Translate the layer in integer canvas pixels without clipping off-artboard content.",
        json!({"layer":{"name":"Background"},"x":-1,"y":2})
    );
    command!(
        "layer.set_fill_opacity",
        FillOpacity,
        Mutate,
        "填充不透明度",
        "Set fill opacity",
        "Set finite content fill_opacity in [0,1]. Layer opacity still controls the final result including styles; fill opacity controls content only.",
        json!({"layer":{"name":"Background"},"fill_opacity":0.5})
    );
    command!(
        "layer.set_locks",
        SetLocks,
        Mutate,
        "图层锁定",
        "Set layer locks",
        "Set transparency/pixels/position/all locks. Full lock blocks edits except visibility and this unlock operation. Pixel operations and source color conversion respect locks at the command boundary.",
        json!({"layer":{"name":"Background"},"locks":{"position":true}})
    );
    command!(
        "layer.set_color_label",
        SetColorLabel,
        Mutate,
        "图层颜色标签",
        "Set layer color label",
        "Set red/orange/yellow/green/blue/violet/gray or null. Does not change pixels.",
        json!({"layer":{"name":"Background"},"color_label":"blue"})
    );
    command!(
        "layer.move_order",
        Order,
        Mutate,
        "图层顺序",
        "Reorder layer",
        "Move a layer to an existing zero-based index, bottom to top.",
        json!({"layer":{"name":"Background"},"index":0})
    );
    command!(
        "layer.select",
        Target,
        View,
        "选择图层",
        "Select layer",
        "Select an existing layer for host interaction; this does not edit pixels or history.",
        json!({"layer":{"name":"Background"}})
    );
    command!(
        "select.marquee",
        Marquee,
        Mutate,
        "矩形选区",
        "Rectangular marquee",
        "Replace, add, subtract or intersect a bounded 16-bit coverage selection. Coordinates are canvas pixels and out-of-artboard area is clipped.",
        json!({"x":0,"y":0,"width":100,"height":100,"mode":"replace"})
    );
    command!(
        "select.subject",
        crate::SubjectParams,
        Mutate,
        "选择主体",
        "Select subject",
        "Run the host's checksum-verified local ONNX subject model. Optional region must be inside the artboard; confidence/multiple-subject ambiguity requires asking the user before accept_ambiguous=true. This does not identify people. Missing assets require explicit host download approval; inference is cancellable and cannot be batched.",
        json!({})
    );
    command!(
        "select.all",
        Empty,
        Mutate,
        "全选",
        "Select all",
        "Select the complete artboard coverage.",
        json!({})
    );
    command!(
        "select.none",
        Empty,
        Mutate,
        "取消选择",
        "Deselect",
        "Remove the current selection.",
        json!({})
    );
    command!(
        "select.inverse",
        Empty,
        Mutate,
        "反选",
        "Inverse selection",
        "Invert current selection coverage inside the artboard.",
        json!({})
    );
    command!(
        "layer.mask.add",
        AddMask,
        Mutate,
        "添加图层蒙版",
        "Add layer mask",
        "Add a sparse 16-bit white, black or current-selection mask. The source pixels remain unchanged.",
        json!({"layer":{"name":"Background"},"source":"white"})
    );
    command!(
        "layer.mask.set_enabled",
        EnableMask,
        Mutate,
        "停用蒙版",
        "Enable layer mask",
        "Enable or disable an existing mask without deleting it.",
        json!({"layer":{"name":"Background"},"enabled":false})
    );
    command!(
        "layer.mask.invert",
        Target,
        Mutate,
        "反相蒙版",
        "Invert layer mask",
        "Invert an existing mask in place while preserving its exact 16-bit samples.",
        json!({"layer":{"name":"Background"}})
    );
    command!(
        "layer.mask.remove",
        Target,
        Mutate,
        "移除蒙版",
        "Remove layer mask",
        "Remove the mask metadata and its coverage plane; source pixels remain unchanged.",
        json!({"layer":{"name":"Background"}})
    );
    command!(
        "layer.add_adjustment",
        AddAdjustment,
        Mutate,
        "新建曝光调整层",
        "Add exposure adjustment",
        "Create a non-destructive exposure adjustment layer with stops in -20..20 and a white, black or selection mask.",
        json!({"name":"人物提亮","type":"exposure","stops":0.5,"mask":"from_selection"})
    );
    command!(
        "adjust.exposure.set",
        SetExposure,
        Mutate,
        "设置曝光",
        "Set exposure",
        "Set exposure adjustment stops in -20..20; source pixels are not rewritten.",
        json!({"layer":{"name":"人物提亮"},"stops":0.5})
    );
    command!(
        "edit.undo",
        Empty,
        History,
        "撤销",
        "Undo",
        "Move the shared document history backward; revision still increases.",
        json!({})
    );
    command!(
        "edit.redo",
        Empty,
        History,
        "重做",
        "Redo",
        "Move forward on the current redo branch; new edits discard that branch.",
        json!({})
    );
    command!(
        "edit.undo_turn",
        UndoTurn,
        History,
        "撤销本轮",
        "Undo turn",
        "Undo a contiguous history tail belonging to one AI turn, including its paused user edits.",
        json!({"turn_id":"00000000-0000-4000-8000-000000000001"})
    );
    command!(
        "document.batch",
        Batch,
        Mutate,
        "批量修改",
        "Atomic batch",
        "Preflight all steps. Only same-document query/mutate steps are allowed; results can be referenced with {$ref:{step:0,field:layer_id}}.",
        json!({"steps":[{"command":"layer.create","params":{"name":"Batch layer"}}]})
    );
    command!(
        "view.pan",
        Pan,
        View,
        "平移画布",
        "Pan canvas",
        "Pan by physical screen pixels; no document history is created.",
        json!({"dx":10.0,"dy":5.0})
    );
    command!(
        "view.zoom",
        Zoom,
        View,
        "缩放画布",
        "Zoom canvas",
        "Zoom around a physical screen pixel. Factor must be finite and positive.",
        json!({"x":100.0,"y":100.0,"factor":1.25})
    );
    command!(
        "view.fit",
        Empty,
        View,
        "适合屏幕",
        "Fit canvas",
        "Fit the current artboard to the viewport without changing document content.",
        json!({})
    );
    command!(
        "view.actual_size",
        Empty,
        View,
        "实际像素 100%",
        "Actual pixels at 100%",
        "Set absolute viewport zoom to one physical display pixel per image pixel, keeping the image-space center unchanged. This is independent of the previous zoom and Retina point scale; unlike view.zoom it is not a multiplicative factor. No document samples, geometry, dirty state or history change. Native navigation and AI use the same command. 中文：实际像素、百分之百、100%、恢复原始缩放。",
        json!({})
    );
    command!(
        "file.open",
        OpenFile,
        Lifecycle,
        "打开文件",
        "Open file",
        "Open an authorized image or Strata project after validation. Dirty replacement requires discard_changes.",
        json!({"resource":"fixture-read"})
    );
    command!(
        "file.recover",
        OpenFile,
        Lifecycle,
        "恢复未保存文档",
        "Recover document",
        "Restore an authorized app recovery checkpoint as a dirty, untitled document. Never overwrite or authorize the original_path. Validate all samples before replacing content; discard_changes is required for a dirty current document.",
        json!({"resource":"fixture-read","discard_changes":true})
    );
    command!(
        "file.place",
        PlaceFile,
        Mutate,
        "置入图片",
        "Place image",
        "Place an authorized image above the active layer without resizing. Convert to the document working ICC and depth; x/y are canvas pixels. One undoable edit; not batchable.",
        json!({"resource":"fixture-read","x":0,"y":0})
    );
    command!(
        "file.save",
        SaveFile,
        External,
        "保存工程",
        "Save project",
        "Save a fixed document snapshot through an authorized write resource. Does not enter undo history.",
        json!({"resource":"fixture-write"})
    );
    command!(
        "file.save_as",
        SaveFile,
        External,
        "工程另存为",
        "Save project as",
        "Atomically publish a standalone project. Overwrite permission comes from the host resource, never the JSON payload.",
        json!({"resource":"fixture-write"})
    );
    command!(
        "file.export",
        ExportFile,
        External,
        "导出图片",
        "Export image",
        "Export PNG/TIFF at document depth or JPEG/WebP at 8-bit. WebP defaults to lossless; webp_lossless=false enables lossy RGB with lossless alpha, quality 1..100 (default 80). JPEG quality is 1..100 (default 90) and requires an 8-bit RGB background in output space. quality is rejected for lossless WebP; webp_lossless is rejected for non-WebP. Optional width/height must both be positive, bounded to 512 MiB output samples; WebP dimensions <=16383. Resize uses linear-light premultiplied area reduction / bilinear enlargement. target_space is optional srgb/display_p3, otherwise keep working ICC. All outputs include document DPI. 16-bit to JPEG/WebP requires allow_quantization=true. Source samples/history/dirty state do not change.",
        json!({"resource":"fixture-export","format":"png"})
    );
    command!(
        "layer.create_group",
        CreateGroup,
        Mutate,
        "新建组",
        "Create group",
        "Create a pass-through compositing group inside the active group or alongside the active layer. Maximum nesting depth 16.",
        json!({"name":"Group"})
    );
    command!(
        "layer.group",
        GroupLayers,
        Mutate,
        "编组",
        "Group layers",
        "Group a contiguous range of unlocked siblings into a pass-through group, preserving sibling z order. Child canvas coordinates remain unchanged.",
        json!({"layers":[{"name":"Background"}],"name":"Group"})
    );
    command!(
        "layer.ungroup",
        Target,
        Mutate,
        "取消编组",
        "Ungroup",
        "Move immediate group children to its parent at the group position. Reset group visibility/opacity/blend/mask before ungrouping; isolated child blend interactions may differ against the parent backdrop.",
        json!({"layer":{"name":"Group"}})
    );
    command!(
        "layer.reparent",
        Reparent,
        Mutate,
        "移动到组",
        "Reparent layer",
        "Move a layer or subtree to an unlocked group or null root, using optional zero-based sibling index. Reject cycles and excessive depth; absolute canvas positions are preserved.",
        json!({"layer":{"name":"Background"},"parent":null,"index":0})
    );
    command!(
        "layer.fill.solid",
        SolidFill,
        Mutate,
        "新建纯色填充",
        "Create solid fill",
        "Create an editable artboard-sized fill using document-depth RGBA. Colors are interpreted in the working ICC. No source pixel blocks are allocated.",
        json!({"name":"Solid","color":[32768,10000,20000,65535]})
    );
    command!(
        "layer.fill.gradient",
        GradientFill,
        Mutate,
        "新建渐变填充",
        "Create gradient fill",
        "Create a linear (angle degrees) or radial fill. start/end are RGBA at document depth; scale 0.01..100. Interpolate in linear light with premultiplied alpha.",
        json!({"name":"Gradient","start":[0,0,0,65535],"end":[65535,65535,65535,65535],"angle":0,"scale":1})
    );
    command!(
        "layer.fill.solid.set",
        SetSolidFill,
        Mutate,
        "编辑纯色填充",
        "Set solid fill",
        "Update a solid fill layer's document-depth RGBA color; undo restores editable parameters.",
        json!({"layer":{"name":"Solid"},"color":[65535,0,0,65535]})
    );
    command!(
        "layer.fill.gradient.set",
        SetGradientFill,
        Mutate,
        "编辑渐变填充",
        "Set gradient fill",
        "Update endpoints, angle, scale and radial mode of an editable gradient fill.",
        json!({"layer":{"name":"Gradient"},"start":[0,0,0,65535],"end":[65535,0,0,65535],"angle":45,"scale":1,"radial":false})
    );
    command!(
        "layer.add_hue_saturation",
        CreateGroup,
        Mutate,
        "新建色相饱和度调整",
        "Add hue saturation",
        "Create a hue/saturation adjustment layer with the current selection mask, or white mask if no selection. HSL is evaluated in encoded Display P3 while compositing stays linear.",
        json!({"name":"Hue"})
    );
    command!(
        "adjust.hue_saturation.set",
        HueSaturationParams,
        Mutate,
        "设置色相饱和度",
        "Set hue saturation",
        "Set hue -180..180 degrees, saturation and lightness -100..100. Original pixels remain unchanged; masks and opacity interpolate the adjusted composite.",
        json!({"layer":{"name":"Hue"},"hue":20,"saturation":10,"lightness":0})
    );
    command!(
        "layer.set_clipping",
        Clipping,
        Mutate,
        "剪贴蒙版",
        "Set clipping mask",
        "Clip this layer to the nearest lower non-clipped sibling's alpha. Clipping chains cannot start without a pixel/fill/group base; adjustment-only bases are rejected. Group boundaries remain explicit.",
        json!({"layer":{"name":"Clip"},"clipped":true})
    );
    command!(
        "layer.rasterize",
        RasterizeTarget,
        Mutate,
        "栅格化",
        "Rasterize layer",
        "Bake an editable fill or isolated group into document-depth pixels, preserving mask appearance, opacity/blend and off-artboard content. Adjustment layers require explicit bake_backdrop=true, consolidating the lower input stack. Undo restores editable structure.",
        json!({"layer":{"name":"Solid"}})
    );
    command!(
        "layer.mask.apply",
        RasterizeTarget,
        Mutate,
        "应用蒙版",
        "Apply layer mask",
        "Multiply a pixel layer's original alpha by its enabled u16 mask without rewriting hidden RGB. Locked pixels/transparency are protected; fills/groups/editable text are rasterized with the mask baked. Applying a text mask explicitly bakes its source/layout/geometry into a pixel layer; native UI labels this Apply and rasterize. One undo restores the editable text and mask. Adjustments require explicit bake_backdrop=true. 中文：应用蒙版；文字层会应用并栅格化，失去可编辑原文，一次撤销可完整恢复。",
        json!({"layer":{"name":"Background"}})
    );
    command!(
        "layer.merge_down",
        Target,
        Mutate,
        "向下合并",
        "Merge down",
        "Bake this subtree and its lower sibling into one pixel layer. Respect locks, preserve outside-artboard content and reject blends that would change final appearance beyond depth tolerance.",
        json!({"layer":{"name":"Upper"}})
    );
    command!(
        "layer.merge_visible",
        Empty,
        Mutate,
        "合并可见",
        "Merge visible",
        "Bake visible composition to a pixel layer while retaining hidden layers. Hidden children are reparented without revealing them. Pixel equivalence checked before commit.",
        json!({})
    );
    command!(
        "layer.flatten",
        Empty,
        Mutate,
        "拼合图像",
        "Flatten image",
        "Bake the visible image and discard all previous layers including hidden ones. Requires explicit destructive-edit approval in AI turns.",
        json!({})
    );
    command!(
        "layer.stamp_visible",
        Empty,
        Mutate,
        "盖印可见",
        "Stamp visible",
        "Add a new pixel layer containing the visible composition, preserving all source layers. This may change accumulated alpha if the original stack remains visible.",
        json!({})
    );
    command!(
        "layer.align",
        AlignLayers,
        Mutate,
        "对齐图层",
        "Align layers",
        "Align unlocked independent layers/subtrees by exact content bounds to the selection or artboard. Modes: left/horizontal_center/right/top/vertical_center/bottom. No selected ancestor/descendant overlap.",
        json!({"layers":[{"name":"Background"}],"mode":"horizontal_center","reference":"artboard"})
    );
    command!(
        "layer.distribute",
        DistributeLayers,
        Mutate,
        "分布图层",
        "Distribute layers",
        "Distribute at least three unlocked layers by horizontal/vertical centers or equal spacing, preserving the endpoint positions. Supports groups and generated fills.",
        json!({"layers":[{"name":"Background"},{"name":"Second"},{"name":"Third"}],"mode":"horizontal_centers"})
    );
    command!(
        "layer.set_styles",
        SetStyles,
        Mutate,
        "图层样式",
        "Set layer styles",
        "Replace the optional drop_shadow, stroke, outer_glow, color_overlay and gradient_overlay settings. All RGBA colors use document depth/ICC. Shadow blur is Gaussian sigma 0..32 and offsets ±256; stroke/glow sizes 0..64. Content fill opacity does not fade effects; overall layer opacity does. Masks are applied before styles; effects are normal source-over except overlay colors' blend modes.",
        json!({"layer":{"name":"Background"},"styles":{"stroke":{"color":[65535,65535,65535,65535],"size":2,"position":"outside"}}})
    );
    command!(
        "edit.copy",
        PixelTarget,
        External,
        "拷贝",
        "Copy layer pixels",
        "Prepare a bounded selection/artboard copy of the active or specified layer. Returns an opaque source-bound clipboard resource; the trusted host must write its payload to the system clipboard and acknowledge completion. No document history.",
        json!({})
    );
    command!(
        "edit.copy_merged",
        PixelTarget,
        External,
        "合并拷贝",
        "Copy merged",
        "Prepare visible merged selection/artboard pixels for an explicitly approved host clipboard write. Hidden layers are not included.",
        json!({})
    );
    command!(
        "edit.cut",
        ClipboardResource,
        Mutate,
        "剪切",
        "Cut pixels",
        "Clear the source alpha only after the host confirms a copy resource was written to the clipboard. The receipt must match this exact source, document state and original layer. Clipboard contents are not reversed by document undo.",
        json!({"resource":"fixture-clipboard"})
    );
    command!(
        "edit.paste",
        ClipboardResource,
        Mutate,
        "粘贴",
        "Paste pixels",
        "Paste an explicitly host-authorized clipboard image as a centered pixel layer, converting ICC and depth. No implicit clipboard reads; resources are source/turn scoped.",
        json!({"resource":"fixture-clipboard"})
    );
    command!(
        "edit.paste_in_place",
        ClipboardResource,
        Mutate,
        "原位粘贴",
        "Paste in place",
        "Paste a host-authorized clipboard image at its recorded canvas origin. If an external clipboard format has no origin, the host records (0,0).",
        json!({"resource":"fixture-clipboard"})
    );
    command!(
        "edit.clear",
        PixelTarget,
        Mutate,
        "清除",
        "Clear pixels",
        "Clear alpha inside the current selection, or the whole active pixel layer if no selection. Preserve hidden RGB and respect pixel/transparency/ancestor locks.",
        json!({})
    );
    command!(
        "edit.clipboard.write_status",
        ClipboardResource,
        Query,
        "剪贴板写入状态",
        "Check clipboard write",
        "Check the trusted host acknowledgement for a source-bound clipboard resource. This never reads system clipboard data or grants a resource.",
        json!({"resource":"fixture-clipboard"})
    );
    command!(
        "layer.via_copy",
        PixelTarget,
        Mutate,
        "通过拷贝新建图层",
        "Layer via copy",
        "Create a pixel layer from the selected area of the active layer without writing to the system clipboard. One undoable edit.",
        json!({})
    );
    command!(
        "layer.via_cut",
        PixelTarget,
        Mutate,
        "通过剪切新建图层",
        "Layer via cut",
        "Copy the selected active pixel-layer area to a new layer and clear source alpha atomically, without changing the system clipboard.",
        json!({})
    );
    command!(
        "layer.background_to_layer",
        PixelTarget,
        Mutate,
        "背景转普通图层",
        "Background to layer",
        "Convert the active pixel background to an ordinary named Layer 0. Existing document pixel layers already have ordinary editable storage.",
        json!({})
    );
    command!(
        "layer.set_pass_through",
        PassThrough,
        Mutate,
        "组穿透模式",
        "Set group pass-through",
        "Choose normal pass-through or isolated group compositing. Pass-through evaluates child blend/adjustment effects against the parent backdrop, then interpolates with the group mask/opacity. Non-normal group blend or group styles use isolated compositing.",
        json!({"layer":{"name":"Group"},"pass_through":false})
    );
    command!(
        "select.ellipse",
        Marquee,
        Mutate,
        "椭圆选框",
        "Elliptical marquee",
        "Select an antialiased ellipse in a canvas pixel rectangle, using 16-bit 4x4 edge coverage. Mode replace/add/subtract/intersect; artboard clips the result. Cancellable, not batchable.",
        json!({"x":2,"y":2,"width":8,"height":8,"mode":"replace"})
    );
    command!(
        "select.polygon",
        PolygonSelection,
        Mutate,
        "多边形套索",
        "Polygon selection",
        "Close a finite 3..4096-point polygon using the even-odd fill rule and 4x4 coverage, in canvas coordinates. Self intersections are supported. Cancellable and bounded by artboard.",
        json!({"points":[[1,1],[12,1],[8,12]]})
    );
    command!(
        "select.lasso",
        PolygonSelection,
        Mutate,
        "自由套索",
        "Freehand lasso",
        "Commit one closed freehand point path as an antialiased even-odd polygon selection. Same bounded coverage and operations as select.polygon.",
        json!({"points":[[1,1],[8,2],[12,8],[3,12]]})
    );
    command!(
        "select.reselect",
        Empty,
        Mutate,
        "重新选择",
        "Reselect",
        "Restore the last nonempty selection snapshot after deselect or replacement. Selection history is document-scoped, undoable and persisted without changing source image samples.",
        json!({})
    );
    command!(
        "select.feather",
        SelectionRadius,
        Mutate,
        "羽化选区",
        "Feather selection",
        "Gaussian feather sigma radius 0..256 pixels; exact separable kernel through sigma 8, variance-matched three-box approximation for larger radii. Outside-artboard coverage is zero. Cancellable; dense scratch must fit the shared budget.",
        json!({"radius":2})
    );
    command!(
        "select.expand",
        SelectionRadius,
        Mutate,
        "扩展选区",
        "Expand selection",
        "Expand the 50% coverage contour by Euclidean radius 0..256 with a soft boundary; preserve stronger original coverage. Artboard bounds remain fixed.",
        json!({"radius":2})
    );
    command!(
        "select.contract",
        SelectionRadius,
        Mutate,
        "收缩选区",
        "Contract selection",
        "Contract the 50% coverage contour by Euclidean radius 0..256, treating outside-artboard samples as empty. Preserve weaker original coverage.",
        json!({"radius":1})
    );
    command!(
        "select.smooth",
        SelectionRadius,
        Mutate,
        "平滑选区",
        "Smooth selection",
        "Smooth coverage using Gaussian sigma radius/2 and a 25%..75% soft threshold, radius 0..256. Radius zero preserves exact coverage.",
        json!({"radius":2})
    );
    command!(
        "select.border",
        SelectionRadius,
        Mutate,
        "选区边界",
        "Selection border",
        "Create an antialiased border of the given full width around the selection's 50% contour, using Euclidean inside/outside distance. Width 0..256; zero returns empty.",
        json!({"radius":2})
    );
    command!(
        "select.transform",
        TransformSelection,
        Mutate,
        "变换选区",
        "Transform selection",
        "Apply affine [a,b,c,d,tx,ty] to coverage only, with inverse mapping of pixel centers and bilinear interpolation. Matrix must be finite/invertible; source pixels stay untouched.",
        json!({"matrix":[1,0,0,1,2,3]})
    );
    command!(
        "select.wand",
        WandSelection,
        Mutate,
        "魔棒",
        "Magic wand",
        "Select colors within an 8-bit-scaled maximum RGBA-channel tolerance (0..255). Sample visible merged working-space color or raw current pixel layer; transparent RGB is ignored. Contiguous uses four-connected flood fill. Seed is in artboard canvas pixels; all four selection modes, cancellation and shared scratch budget apply.",
        json!({"x":4,"y":4,"tolerance":32,"contiguous":true,"sample":"merged"})
    );
    command!(
        "select.quick",
        QuickSelection,
        Mutate,
        "快速选择",
        "Quick selection",
        "Seeded Gaussian color graph cut on a pixel-center preview of at most 256 pixels on the longest edge. Foreground/background are canvas-coordinate brush points, radius 0.5..256 pixels, smoothness 0..100. Without background strokes, unmarked preview borders supply background. Conflicting preview seeds are rejected. Bilinear upsampling produces u16 coverage. Sample merged or raw current pixel layer; source pixels stay unchanged. Cancellable and not batchable; this is traditional segmentation, not semantic ML.",
        json!({"foreground":[[8,8]],"background":[[1,1]],"radius":1,"sample":"current_layer"})
    );
    command!(
        "layer.add_tonal_adjustment",
        AddTonal,
        Mutate,
        "新建调色调整层",
        "Add tonal adjustment",
        concat!(
            "Create a typed adjustment with the current selection as mask (white if none). Floating encoded document working RGB, source samples and alpha preserved. brightness_contrast: -100..100, additive brightness /100 then contrast about 0.5 (negative gain 1+c; positive 1/(1-c); +100 threshold). levels: master then RGB, normalized black/white and gamma 0.1..10. curves: master then RGB PCHIP, 2..64 points, x strictly increasing from 0 to 1, y 0..1. vibrance: vibrance/saturation -100..100, prioritize low saturation and reduce enhancement near skin hue. color_balance: shadows/midtones/highlights RGB axes -100..100, Rec.709 encoded-luma range weights; preserve_luminance gamut-clips while retaining luma. black_white: six hue sensitivities R/Y/G/C/B/M -200..300, optional normalized tint and tint_amount 0..100. photo_filter: normalized RGB transmittance color, density 0..100 and optional luma preservation. channel_mixer: 3x4 normalized coefficients -2..2 with constants, optional monochrome gray row. invert: 1-RGB. posterize: 2..65536 levels/channel. threshold: encoded Rec.709-weighted level 0..1, equality maps white. gradient_map: 2..64 strictly ordered normalized stops spanning 0..1, encoded RGB interpolation and optional reverse. selective_color: nine CMYK percentage rows R/Y/G/C/B/M/white/neutral/black, -100..100; relative scales by channel/headroom and absolute uses direct offsets. desaturate: HSL lightness (min+max)/2. Neutral settings of brightness/levels/curves/vibrance/balance/mixer/selective color are exact identity in normal mode. One undoable edit; manifest v4.",
            " white_balance: temperature/tint -100..100; positive temperature warms using reciprocal-Kelvin shift relative to D65 (6504K) and Bradford adaptation in linear Display P3, positive tint reduces the M cone response toward magenta; optional physical Y preservation. White balance and local tone are exceptions to encoded working RGB operations. shadows_highlights: shadow/highlight amounts 0..100, widths 0.01..1, radius 0..256 (sigma=radius/3). Physical linear P3 Y and alpha-normalized three-box smoothing estimate local tone on the actual lower input stack. The finite kernel support defines the cross-tile halo; artboard edges clamp to the nearest valid sample. Linear gain is up to two stops in shadows/highlights, then mask/opacity/blend apply. Both strengths zero is exact identity; cancellation and shared temporary budgets apply. 中文检索：亮度、对比度、色阶、曲线、自然饱和度、色温、色调、色彩平衡、黑白、照片滤镜、通道混合器、反相、色调分离、阈值、渐变映射、可选颜色、去色、阴影、高光。"
        ),
        json!({"name":"Levels","adjustment":{"type":"levels","master":{"gamma":1.2}}})
    );
    command!(
        "adjust.tonal.set",
        SetTonal,
        Mutate,
        "设置调色调整层",
        "Set tonal adjustment",
        concat!(
            "Replace complete tonal settings of an existing adjustment layer without changing its type. Same strict normalized parameters as layer.add_tonal_adjustment. Omitted fields reset to that type's defaults; specify the complete intended settings. Masks, alpha and original source samples are preserved; lock and ancestor checks apply.",
            " 中文检索：亮度、对比度、色阶、曲线、自然饱和度、色温、色调、色彩平衡、黑白、照片滤镜、通道混合器、反相、色调分离、阈值、渐变映射、可选颜色、去色、阴影、高光。"
        ),
        json!({"layer":{"name":"Levels"},"adjustment":{"type":"levels","master":{"input_black":0.1,"input_white":0.9,"gamma":1.1}}})
    );
    command!(
        "paint.stroke",
        BrushStroke,
        Mutate,
        "画笔笔触",
        "Brush stroke",
        "Paint one whole stroke on an explicit pixel layer or existing u16 mask. Points are finite canvas x/y and pressure 0..1 (default 1), at most 8192. Size 0.5..4096 canvas pixels; hardness/opacity/flow/value 0..1; spacing 0.01..2 diameters. Round tip, smooth soft falloff and antialiased boundary. Arc-length dab placement is independent of input event density; pressure_size scales diameter to at least 0.25px and pressure_opacity caps local accumulated coverage. Flow accumulates per dab, whole-stroke opacity caps the result. Color is normalized straight encoded document working RGBA. Pixel color blends in linear P3, preserving untouched source samples exactly; transparency lock preserves original alpha and excludes transparent pixels. Mask target blends toward normalized value, respects linked mask anchors and does not alter source pixels. Clip to artboard, current u16 selection, and existing mask bounds. Pixel/all and ancestor locks apply. At most one million dabs, 250 million bounded dab-pixel visits and 250 million tile-dab tests, shared transient/source budget and cancellation. Atomic, one history entry, not batchable. 中文：画笔、绘画、蒙版绘制、压感、流量、硬度、间距。",
        json!({"layer":{"name":"Background"},"points":[{"x":3.5,"y":3.5},{"x":10.5,"y":10.5,"pressure":0.5}],"size":4,"color":[1,0,0,1],"hardness":0.8,"opacity":0.7,"flow":0.5})
    );
    command!(
        "paint.erase",
        BrushStroke,
        Mutate,
        "橡皮擦笔触",
        "Eraser stroke",
        "Erase one arc-length round brush stroke with the same strict geometry/pressure/coverage parameters as paint.stroke. Pixel target changes alpha only and keeps hidden RGB codes exactly; transparency lock rejects erasing. Mask target paints toward normalized value (default white=1); the native eraser uses background-color luminance. Color is ignored. Current selection, linked mask bounds, pixel/ancestor locks, cancellation, shared memory and work limits apply. Atomic one history entry; not batchable. 中文：橡皮擦、擦除、蒙版绘制。",
        json!({"layer":{"name":"Background"},"points":[{"x":4.5,"y":4.5},{"x":11.5,"y":8.5}],"size":4,"opacity":0.5})
    );
    command!(
        "paint.sample_color",
        SampleColor,
        Query,
        "吸取颜色",
        "Sample color",
        "Read encoded working RGBA at an artboard canvas point; sample merged visible composite or visible active raw pixel layer. Radius 0..31; larger neighborhoods are alpha-weighted in linear P3, then converted through the actual working ICC; output alpha is mean alpha. Raw current-layer radius zero preserves exact source-depth values including hidden RGB. Query does not edit document/history/clipboard. 中文：吸管、取色、颜色取样。",
        json!({"x":4,"y":4,"radius":0,"sample":"merged"})
    );
    command!(
        "paint.fill",
        FillPixels,
        Mutate,
        "填充像素",
        "Fill pixels",
        "Fill an explicit unlocked pixel layer inside the artboard and current u16 selection, normalized working RGBA and opacity 0..1. Straight colors convert through actual working ICC and composite in linear P3; source samples at zero selection coverage remain exact. Transparency lock preserves alpha/excludes transparent pixels; ancestor locks, cancellation, shared tile budgets and one atomic history apply. Not batchable. 中文：填充、前景色、背景色。",
        json!({"layer":{"name":"Background"},"color":[0.2,0.4,0.8,1],"opacity":0.5})
    );
    command!(
        "paint.bucket",
        BucketPixels,
        Mutate,
        "油漆桶",
        "Paint bucket",
        "Compute wand-style tolerance 0..255 and four-connected/global matches from merged or current-layer working samples, then fill the explicit pixel target through that region and current selection. Sample/seed and transparency semantics match select.wand; fill/locks/budget/history match paint.fill. Sampling leaves the current selection unchanged. Cancellable and not batchable. 中文：油漆桶、连续、容差。",
        json!({"layer":{"name":"Background"},"x":4,"y":4,"color":[1,0.2,0,1],"tolerance":32,"contiguous":true})
    );
    command!(
        "paint.gradient",
        GradientPixels,
        Mutate,
        "渐变像素",
        "Paint gradient",
        "Paint linear/radial/angle/reflected/diamond gradient on an explicit pixel layer, artboard and current selection. Start/end are bounded canvas positions and must differ. At most 64 strictly increasing stops spanning at=0..1; normalized working RGBA. Colors interpolate in premultiplied encoded working RGB to avoid transparent hidden-color halos, then composite in linear P3 with opacity 0..1; reverse optional. Shape coordinates use pixel centers: linear projection, radial distance, signed angle modulo one turn, reflected absolute projection, diamond L1 distance. Fill locks/cancellation/shared budgets/atomic history apply. 中文：线性、径向、角度、对称、菱形渐变。",
        json!({"layer":{"name":"Background"},"start":[2.5,8.5],"end":[12.5,8.5],"stops":[{"at":0,"color":[0,0,0,1]},{"at":1,"color":[1,1,1,1]}],"shape":"linear"})
    );
    command!(
        "retouch.clone",
        BrushStroke,
        Mutate,
        "仿制图章笔触",
        "Clone stamp stroke",
        "Clone from an immutable input snapshot during one pixel brush stroke. Required clone_source.source is canvas [x,y]; destination is the canvas anchor for fixed alignment, default first stroke point. Sampling maps target + source - destination, bilinear premultiplied linear P3 with out-of-artboard transparent; sample=current_layer uses raw target pixel layer, sample=merged uses the visible composite. Source colors never feed back from just-written target samples. Same size/hardness/opacity/flow/spacing/pressure/selection/transparency and ancestor lock semantics as paint.stroke. Pixel target only; color/value ignored, clone_source not allowed on ordinary brushes. At most 8192 points, bounded dab work and tile cache; shared budgets and cancellation apply. Atomic one history edit. 中文：仿制图章、取样点、对齐、所有图层取样。",
        json!({"layer":{"name":"Background"},"points":[{"x":10.5,"y":10.5}],"size":4,"clone_source":{"source":[3.5,3.5],"sample":"current_layer"}})
    );
    command!(
        "retouch.heal",
        HealStroke,
        Mutate,
        "修复画笔笔触",
        "Healing brush stroke",
        "Transfer source texture while solving linear-P3 Poisson gradients against an immutable target Dirichlet boundary. Required stroke uses the same round brush/selection/pressure/opacity/flow/locks as paint.stroke, pixel target and explicit clone_source as retouch.clone; color/value ignored. source/current_layer requires nontransparent target boundary; merged sampling can use the visible composite for source and destination reference. Transparent source or destination samples are excluded from the solved domain. Footprint plus 1px boundary at most 262144 pixels. Conjugate gradient max_iterations 1..2048 (default 512), RMS tolerance 1e-7..0.001 (default 0.00005); nonconvergence returns error without editing. Soft coverage and selection applied once, source samples unchanged outside it. Finite solved colors clamp to working RGB gamut at publication; source alpha blends over the target. Shared memory and cancellation throughout sampling/solve/publish; one atomic history item, not batchable. This is sampled gradient-domain healing; automatic spot matching is a separate command. 中文：修复画笔、源纹理、边界融合。",
        json!({"stroke":{"layer":{"name":"Background"},"points":[{"x":10.5,"y":10.5}],"size":4,"clone_source":{"source":[3.5,3.5]}},"max_iterations":512,"tolerance":0.00005})
    );
    command!(
        "retouch.spot_heal",
        SpotHealStroke,
        Mutate,
        "污点修复笔触",
        "Spot healing stroke",
        "Automatically match nearby intact texture outside a round brush footprint, then apply retouch.heal Poisson boundary fusion. Nested stroke has a pixel target and no clone_source; brush/selection/locks/pressure/flow/opacity and atomic one-history semantics remain. Sample current_layer (default raw target) or merged visible composite. Search radius 1..512px (default 64), deterministic coarse translation grid with local refinement, at most 2048 offsets and 128 nontransparent boundary/32 interior samples; search decodes at most 1024 source tiles and caches eight. Source and destination footprint bounding rectangles plus boundary never overlap and source stays inside artboard. Score is boundary linear-P3 RGB variance after mean offset plus 5% mean-color error; nearest offset breaks numerical ties. Return chosen canvas source anchor, candidate count, RMS boundary error and healing diagnostics. Footprint plus 1px boundary <=262144 pixels; max_iterations/tolerance follow retouch.heal. Missing usable source, nonconvergence, cancellation or shared memory failure do not edit the document. This is proximity texture matching; content-aware PatchMatch synthesis is tracked separately. 中文：污点修复、自动近邻纹理匹配、边界融合。",
        json!({"stroke":{"layer":{"name":"Background"},"points":[{"x":11.5,"y":8.5}],"size":2,"hardness":1},"search_radius":8})
    );
    for (id, zh, en) in [
        ("retouch.dodge", "减淡笔触", "Dodge stroke"),
        ("retouch.burn", "加深笔触", "Burn stroke"),
    ] {
        command!(
            id,
            TonalBrushStroke,
            Mutate,
            zh,
            en,
            "Local dodge/burn on an explicit unlocked pixel layer. Nested stroke has round brush coverage/pressure/flow/opacity/selection/locks and no clone_source; target=pixels, color/value ignored. exposure 0..1 (default .5), range shadows/midtones/highlights (default midtones). Range uses physical linear P3 Y and CIE L*: shadows=1-smoothstep(.05,.65,L), highlights=smoothstep(.35,.95,L), midtones=4L(1-L). Gain=2^(+/-2*exposure*rangeWeight). protect_tones=true (default) applies Y'=Y/(Y+(1-Y)/gain), preserves black/white endpoints and compresses chroma in the actual working-ICC gamut toward Y'; false applies/clamps linear RGB gain. Finite u16 coverage blends transformed color once; alpha and zero-coverage/transparent hidden samples remain exact. Actual working ICC and 8/16-bit retained, transient tile budget/cancellation and one atomic history item. 中文：减淡、加深、曝光量、阴影、中间调、高光、保护色调。",
            json!({"stroke":{"layer":{"name":"Background"},"points":[{"x":8.5,"y":8.5}],"size":4,"hardness":1},"exposure":0.5,"range":"midtones","protect_tones":true})
        );
    }
    command!(
        "retouch.sponge",
        SpongeStroke,
        Mutate,
        "海绵笔触",
        "Sponge stroke",
        "Local saturation/desaturation through one nested round brush stroke, explicit pixel target and no clone_source; color/value ignored. mode=saturate/desaturate (default desaturate), amount 0..1 (default .5), vibrance=true by default. Scale linear-P3 RGB chroma about physical Y by 1+/-strength; strength=amount/(1+4*maxAbsChroma) with vibrance, amount otherwise. Compress chroma in the actual working-ICC gamut toward Y (matrix/TRC fast path, unquantized ICC round-trip fallback), preserve physical luminance and exact source alpha; transparent/zero-coverage samples remain exact. Same selection/pressure/opacity/flow/locks, actual working ICC, cancellation/shared tile budget and one atomic history item as dodge/burn. 中文：海绵、增加饱和度、降低饱和度、自然饱和度。",
        json!({"stroke":{"layer":{"name":"Background"},"points":[{"x":8.5,"y":8.5}],"size":4},"mode":"desaturate","amount":0.5,"vibrance":true})
    );
    command!(
        "filter.gaussian_blur",
        GaussianBlur,
        Mutate,
        "高斯模糊",
        "Gaussian blur",
        "Blur an explicit unlocked pixel layer using an immutable input snapshot, true normalized separable Gaussian kernel, sigma 0.1..64 canvas pixels (default 2), radius ceil(3*sigma). Linear P3 premultiplied RGBA, double-precision accumulation; actual working ICC and 8/16-bit source retained. Clamp sampling at the artboard boundary; missing pixels inside artboard are transparent. Current u16 selection limits publication only, never the input halo; existing masks unchanged. Transparency lock keeps original alpha and excludes fully transparent target pixels. Cache one finite output tile; halo at most 640x640, per-tile/global scratch reservations and row cancellation. Zero-coverage samples preserve exact source codes. One atomic history edit; not batchable. 中文：高斯模糊、模糊背景、柔化、半径、标准差。",
        json!({"layer":{"name":"Background"},"sigma":2})
    );
    command!(
        "image.auto",
        AutoCorrect,
        Mutate,
        "自动色调 / 对比度 / 颜色",
        "Automatic tone contrast and color",
        "Automatically correct an explicit visible unlocked pixel layer (visible ancestors required, so hidden-layer source statistics are not disclosed), using immutable raw working-RGB source inside the artboard and current selection. settings.method=contrast pools all three RGB histograms to derive one shared black/white stretch; tone derives each RGB channel stretch independently; color adds a midtone-neutralization gamma per channel. clip_percent0..5 (default .5) is removed independently from each tail using exact65536-bin encoded source-code histograms weighted by u16 selection*source alpha; integer totals/quantiles never lose precision through an8-bit preview or floating total. Upper weighted median estimates neutral midtones for color: t=(median-black)/(white-black), target_gamma=clamp(log(t)/log(.5),.1,10) if0<t<1, otherwise1; gamma=target_gamma^neutralize_strength, strength0..1(default1). Stretch then pow(value,1/gamma); flat-range channels stay exact identity. This gray-world median assumption can alter intentional color casts; strength0 gives the same correction as tone. All formats use actual encoded working RGB, not an Adobe proprietary algorithm equivalence claim. Full selection writes mapped8/16-bit RGB codes directly with one nearest-code quantization (half-code ties within1e-9 code round upward consistently); soft selection blends original/transformed colors once in linear P3 through the actual ICC. Preserve original alpha, alpha-zero hidden RGB, zero-selection/off-artboard samples, layer masks/definitions/styles/structure/ICC/depth. Layer/ancestor pixel/all locks reject; transparency/position locks allowed because alpha/geometry do not change. No nonpixel implicit rasterization, no visible-composite/other-layer input. At most64M sample visits, bounded exact histograms/LUTs and per-tile pooled scratch/output, row/quantile/LUT cancellation, one atomic history and exact undo. No eligible selected opaque samples or no actual code change returns changed=false with no revision/history/dirty/current-layer change. Not batchable; exact AI pixels.replace approval. Shared native/AI command and one-slot cancellable original-context display preview. 中文：自动色调、自动对比度、自动颜色、自动校正、端点裁切、分通道对比度、中间调中和、灰世界、当前像素层。",
        json!({"layer":{"name":"Background"},"settings":{"method":"tone","clip_percent":0.5}})
    );
    command!(
        "filter.high_pass",
        HighPass,
        Mutate,
        "高反差保留",
        "High pass",
        "Retain fine detail on an explicit unlocked pixel layer: compute clamp(0.5 + source - Gaussian(source),0,1) per RGB channel, preserving source alpha. space=working_rgb(default) operates in encoded document-working RGB and produces 50% working gray in flat areas; space=linear_p3 operates in linear Display P3 and centers flat areas on0.5 linear light, the neutral point for Strata overlay/soft-light/linear-light blending. Choose linear_p3 for overlay sharpening without darkening flat areas; working gray is not the neutral point of those linear blend modes. sigma 0.1..64 canvas pixels (default2), radius ceil(3*sigma); normalized separable Gaussian with double-precision alpha-weighted accumulation and unpremultiplication, so transparent hidden RGB never creates fringes. Low frequency and subtraction use the same chosen space; filter.gaussian_blur remains linear light. Both modes are documented algorithms, not an Adobe proprietary equivalence claim. Clamp sampling at artboard edges; missing source inside artboard is transparent. Use immutable raw layer input, not the visible composite or the selection-clipped input. Current u16 selection affects publication once in linear P3 through the actual ICC. Preserve exact source alpha, fully transparent hidden RGB and all zero-selection/off-artboard codes, masks/styles/structure/ICC and8/16-bit depth; RGB is quantized only at final publication with normal ICC floating precision. Pixel/all or ancestor locks reject, position/transparency locks allowed; nonpixel targets reject without rasterization. At most64M pixels in the artboard-clipped selection bounding rectangle; one cached output tile and bounded halo at most640x640, pooled scratch/source/output budgets and row cancellation. An unchanged final code tile is not published; touched_tiles=0 has no history/revision/dirty/current-layer change. One atomic history/undo, not batchable, exact AI pixels.replace approval. Native parameters and cancellable one-slot original-context preview use this same command. 中文：高反差保留/高通滤镜从原像素减去同空间的透明度加权高斯低频，再加中性值并截到0–1。工作RGB模式得到50%编码灰；线性P3模式适用于叠加锐化，在叠加/柔光/线性光混合中保持平坦区域中性。标准差0.1–64像素，保留原透明度、隐藏RGB、选区外原码和蒙版，选区软边只混合一次。实际变化可一次撤销，零变化无历史；逐次像素替换授权与原生参数/预览同源。关键词：保留细节、灰底纹理、边缘细节、线性高反差、50%灰。",
        json!({"layer":{"name":"Background"},"sigma":2})
    );
    command!(
        "filter.unsharp_mask",
        UnsharpMask,
        Mutate,
        "USM 锐化",
        "Unsharp mask",
        "Sharpen an explicit unlocked pixel layer from immutable input and the same alpha-normalized Gaussian low frequency as filter.gaussian_blur. sigma 0.1..64 (default 2), amount 0..5 (default 1), threshold 0..1 linear P3 RGB (default 0). If every channel absolute source-minus-blur delta is below threshold, preserve that pixel; otherwise add amount*delta and clamp linear P3 RGB 0..1 before working-ICC publication. Preserve source alpha exactly and ignore transparent hidden RGB. amount=0 is an exact identity. Selection limits writes, masks/locks/offsets/8-16-bit/cancellation/shared memory match Gaussian. Atomic one history edit, not batchable. Threshold is explicitly linear RGB, not an Adobe encoded threshold claim. 中文：USM、反差锐化、锐化、数量、阈值。",
        json!({"layer":{"name":"Background"},"sigma":1,"amount":1.2,"threshold":0.01})
    );
    command!(
        "filter.add_noise",
        AddNoise,
        Mutate,
        "添加杂色",
        "Add noise",
        "Add deterministic noise to raw encoded working RGB on an explicit unlocked pixel layer. amount 0..1 (default .02): maximum absolute amplitude for distribution=uniform (default), standard deviation for gaussian. u32 seed (default 0), monochrome=false by default. Coordinate/channel SplitMix64 hashing and Box-Muller Gaussian, independent of tile traversal; monochrome shares the same encoded perturbation across RGB. Clip working RGB 0..1 then use floating actual ICC for linear-P3 coverage blend; no 8-bit intermediate. Preserve exact alpha, transparent hidden RGB and zero-selection codes. amount=0 exact identity; masks/locks/cancel/global tile budgets and one atomic history item. Not batchable. 中文：添加杂色、添加噪点、颗粒、胶片颗粒、均匀、高斯、单色、随机种子。",
        json!({"layer":{"name":"Background"},"amount":0.02,"distribution":"gaussian","monochrome":true,"seed":42})
    );
    command!(
        "layer.pixel_bounds",
        Target,
        Query,
        "像素图层边界",
        "Pixel layer bounds",
        "Read finite occupied canvas bounds of an explicit visible pixel/text layer or visible group, including stored transparent hidden RGB. Sparse tiles scan with cancellation; query never changes samples/history. An editable text layer with no stored RGBA codes (for example fully transparent black) uses its retained glyph coverage, geometry and crop to measure ink instead; this bounded source-only fallback reserves the same font/mask memory and supports cancellation without writing a render cache. Whitespace/fully deleted ink still reports null. Group bounds union only effective visible child source, including finite fills, and never disclose hidden descendant extent. Scan at most64M stored child-source visits; masks/effects do not enlarge this raw-content box. Invisible target/ancestor or other unsupported target rejected. Null means no stored content. Use before transform.apply to choose source corners or pivot; coordinates include layer offset. Not batchable. 中文：像素图层边界、文字图层边界、图层组边界、可见子层范围、内容尺寸、变换轴心。",
        json!({"layer":{"name":"Background"}})
    );
    command!(
        "transform.apply",
        TransformApply,
        Mutate,
        "像素自由变换",
        "Pixel layer or selected-pixel projective transform",
        "Transform an explicit pixel/text layer or supported group subtree and linked u16 masks from immutable source, preserving canvas-external content, selection and styles. Required scope=whole_layer or selection. whole_layer transforms all stored pixels and linked mask, leaving selection unchanged. selection requires nonempty document coverage: lift nontransparent source once with u16 selection into 16-bit temporary tiles, keep a source-over complement r=(a-s)/(1-s) with s=selected alpha, warp that immutable lift, then composite over remainder in premultiplied linear light. This complement reconstructs identity alpha and differs from simply multiplying original alpha by (1-coverage). Never clip the destination again by old selection. Move current selection through the same geometry/interpolation, clip coverage to the artboard, store old selection for reselect; keep layer position/styles/linked mask unchanged in selection mode. Zero-coverage and original alpha-zero hidden RGB remain exact at source; selected fully opaque coverage carries exact visible codes. Identity transform is exact and returns changed=false with no history/revision/dirty/current-layer change, in both scopes; identical valid source/destination corners normalize to exact identity. Nonidentity operations return changed=true. Pixel destinations outside artboard stay stored even if selection outline clips away. RGB lifting/warp and destination composite remain bounded/cancellable/pooled; no 8-bit intermediate for soft coverage. Editable text supports whole_layer with all geometry variants, including perspective, and retains Unicode/style plus a local geometry pipeline (manifest8). Reshape original source and transform uniform-color glyph coverage; adjacent transforms compose before sampling and crop barriers stay ordered. Exact nearest lattice operations preserve glyph codes; bilinear interpolates u16 coverage without RGB/ICC round trips. Re-edit preserves geometry. Selection scope rejects text: explicitly rasterize first or choose whole_layer. Layer storage position rebases to transformed content. Empty text ink rejects. At most32 retained stages and64M transformed coverage visits per text re-render; glyph/source/shared-memory limits still apply. Groups support whole_layer when every subtree item is pixel, editable text or group with no active styles. One geometry applies to all children including hidden descendants and every linked/disabled layer/group mask, preserving hierarchy/clipping/IDs/source text. Preflight all subtree and ancestor pixel/position/transparency/all locks, types, source64M and aggregate64M destination pixel/mask work. Integer translation changes only subtree positions and retains all tile versions. General operations transform each editable source through the same leaf sampler, never flatten; empty text retains geometry for later typing. Group storage origin becomes transformed visible-content logical origin. Query/handle/result bounds and mask flag use only effective visible subtree; hidden content still transforms without revealing its extent. An entirely invisible/empty group or group selection scope rejects. Generated fill/adjustment/active-style general geometry remains unsupported; never rasterize/drop them implicitly. Native canvas handles now stage the same corners payload with fixed original document/revision/control/layer/scope ownership: eight scale handles, center-based rotation, skew, distort, paired-corner perspective, translation and keyboard nudge. Shift constrains ratio/direction/15-degree rotation, Option scales about center. Pointer motion previews only the quad; Enter applies once and Escape cancels. Continuous warped-image feedback remains separate; the existing numeric panel can still display a pixel preview. geometry.type=components: translation [x,y], scale [sx,sy] (negative flips), rotation_degrees, shear_degrees [x,y], pivot canvas [x,y]; order scale, shear, rotation, translation around pivot. affine values [a,b,c,d,tx,ty]. matrix values row-major [a,c,tx,b,d,ty,p,q,w], projective division. corners maps four cyclic convex source corners to destination corners; canvas coordinates, reversed orientation allowed. Use layer.pixel_bounds for actual source extent. interpolation nearest/bilinear (default): nearest floating edge ties within 1e-9 source pixel choose the positive-side sample; integer centers keep exact codes, fractions premultiplied linear P3 with actual ICC. Empty pixel layers reject. Exact integer translation only changes the layer position and keeps source/mask tile versions; signed axis permutations use integer pixel-center mapping even at large canvas offsets. Other output tiles are rebased to the transformed logical bounds, and the layer position reports that origin. Reject singular/horizon/unbounded or >64M-output visits; independent RGB/mask eight-tile/4096-decode caches, shared reservations/row cancellation. Reject pixel/position/transparency/all and ancestor locks. Linked mask flag preserved, anchors rebound. One atomic history, not batchable. 中文：自由变换、图层组变换、嵌套组旋转、可编辑文字变换、保留文字源旋转、整层像素、选区内像素、移动选中区域、缩放、旋转、斜切、扭曲、透视、水平翻转、垂直翻转、画布手柄、八手柄、旋转圆点、四角草稿、等比缩放、中心缩放、回车应用、Esc取消；画布拖动显示轮廓，应用时用同一命令更新像素，恒等变换不写历史。",
        json!({"layer":{"name":"Background"},"scope":"whole_layer","geometry":{"type":"components","scale":[-1,1],"pivot":[8,8]},"interpolation":"nearest"})
    );
    command!(
        "image.canvas_size",
        CanvasSize,
        Mutate,
        "画布大小",
        "Canvas size",
        "Resize the artboard only; no resampling, flattening, source deletion or movement of stored layer/mask coordinates. width/height integer 1..100000 subject to the document byte budget. anchor top_left/top/top_right/left/center(default)/right/bottom_left/bottom/bottom_right fixes the corresponding old canvas anchor; center uses floor division for odd differences. Source pixels (including hidden RGB), linked/disabled masks, hierarchy, clipping, styles and locked layers stay editable outside the new frame. Frozen generated-fill/style dimensions and spatial-adjustment sampling domains preserve original appearance when the canvas changes; extension is transparent except for already existing outside content. Current/previous u16 selections clip to the new artboard and expand with zero coverage, with at most 64M selection-copy visits. Does not fill the extension with a new background. Atomic cancellable single history step; manifest v6 persists frozen references/crop support and reads legacy v0..v5. Not batchable. 中文：画布大小、扩展画布、缩小画布、锚点、保留画板外像素、不要缩放图像。",
        json!({"width":32,"height":24,"anchor":"center"})
    );
    command!(
        "image.crop",
        CropCanvas,
        Mutate,
        "矩形裁剪",
        "Crop rectangle",
        "Crop the artboard to an explicit canvas-coordinate rect {x,y,width,height}; required pixels=preserve or delete. Preserve keeps source codes/layers/linked masks outside the crop. Delete explicitly clears all outside RGBA codes (including hidden/transparent RGB), drops fully outside tiles, retains unchanged inside tile versions/boundary codes, clips linked/disabled masks to finite crop coverage, and restricts generated fill support without rasterizing or changing original gradient/style reference axes. Empty generated intersection stays explicitly empty; expansion/mask removal cannot regenerate deleted support. Delete affects hidden layers too and rejects affected pixel/transparency/all locks; position-only lock allowed. AI requires exact delete approval even in Direct mode. Both modes retain IDs/tree/clipping/editable definitions/ICC/depth with one atomic undo. Local generated finite crop persists in manifest v6, reading legacy v0..v5. Optional aspect_ratio [w,h], each 1..100000, reduces to coprime integers; largest exact integer-ratio rectangle inside rect anchored at top-left, too-small box rejects. Same canvas byte/dimension, finite current/previous selection clipping/zero extension, frozen references, shared budgets and row cancellation apply; delete boundary work capped at 64M visits. Native border-only drag with eight handles/inside movement binds initial document/revision/control epoch; Enter applies this command, Esc cancels without history, changed context rejects late confirmation. Tokens remain trusted host state, never model parameters. Selection is not a crop source/write clip. Straighten/perspective crop remain unsupported; unknown angle fields reject. Not batchable. 中文：矩形裁剪、比例裁剪、保留裁剪像素、删除裁剪像素、删除画板外原码；保留结构与渐变定义，明确选择是否删除像素。",
        json!({"rect":{"x":2,"y":3,"width":12,"height":10},"pixels":"preserve","aspect_ratio":[4,3]})
    );
    command!(
        "image.reveal_all",
        Empty,
        Mutate,
        "显示全部可见图层内容",
        "Reveal visible stored layer bounds",
        "Expand the current artboard to include all effectively visible pixel and generated-fill source bounds plus conservative own/ancestor style outsets. Never shrink the existing frame; keep absolute layer positions, source samples, masks, locked hierarchy and editable definitions. Pixel extents include stored transparent hidden RGB; masks/clipping/zero opacity can make the bounds conservative rather than minimal rendered-alpha bounds. Invisible layers/ancestors do not contribute; no hidden-layer metadata is returned. Uses the same frozen-reference and zero-extended selection path as image.canvas_size. Reject unsupported/overflowing/over-budget canvas sizes atomically, cancellable source scans, one history step, not batchable. This changes document geometry; view.fit is a separate display action. 中文：显示全部、恢复画板外内容、扩展到所有可见源图层；保留现有画板并扩展到内容与样式边界，不移动或重采样图层。",
        json!({})
    );
    command!(
        "image.orient",
        OrientImage,
        Mutate,
        "图像旋转与翻转",
        "Rotate or flip the layered image",
        "Apply a whole-document exact signed-axis permutation: operation rotate_90_cw, rotate_90_ccw, rotate_180, flip_horizontal or flip_vertical. Keep the artboard top-left fixed; quarter turns swap width/height. Transform all stored source pixels including off-artboard/hidden layers and transparent hidden RGB, all linked/disabled u16 masks and current/previous selections. Preserve hierarchy, clipping, IDs, working ICC/depth and editable pixel/fill/group/adjustment definitions. Rotate/reflect gradient and gradient-overlay axes, swap frozen dimensions, map spatial adjustment domains and shadow offset vectors; radius/scale stay unchanged under these isometries. No rasterization or source artboard clipping. Exact nearest lattice sampling preserves integer codes. Existing procedural dissolve still uses its deterministic destination canvas-coordinate pattern, not a rotated random bitmap. Reject any layer pixel/position/transparency/all lock, unrepresentable/out-of-range geometry, >64M raster/mask/reference bound visits or shared memory/cache limits; atomic cancellation and one-history undo. All current layer kinds supported, including retained-source text: glyph coverage and ordered crop/geometry transform while Unicode/style remain editable. Future smart objects need their own support. Not batchable; AI requires exact content-replacement approval. Arbitrary-angle image rotation/straighten uses its separate partial-type commands. 中文：整幅图像旋转、顺时针90度、逆时针90度、180度、图像水平翻转、图像垂直翻转；同步保留可编辑图层、蒙版、选区与画板外内容。",
        json!({"operation":"rotate_90_cw"})
    );
    command!(
        "paint.colors.get",
        Empty,
        Query,
        "读取前景色与背景色",
        "Read foreground/background colors",
        "Read this editor's tool palette: normalized encoded working-RGB foreground/background RGBA 0..1 and a palette version. The current document working ICC interprets these codes (sRGB when no document); palette is per editor/window, starts opaque black/white, survives document replacement in that editor, and is not project metadata/history/recovery content. Reading never changes pixels, document revision/dirty/history or tool state. Native swatches, eyedropper and X/D use the same palette. Painting commands still accept explicit working-RGBA color; use the returned foreground/background as that parameter when desired. Allowed without document, bound source/session/control checked; not batchable. 中文：读取前景色、读取背景色、当前绘画颜色、工具调色板。",
        json!({})
    );
    command!(
        "paint.colors.set",
        SetPaintColors,
        View,
        "设置前景色与背景色",
        "Set foreground/background colors",
        "Set foreground and/or background in this editor's tool palette as finite normalized encoded working-RGB RGBA 0..1. At least one color required; omitted side remains. Optional expected_version from paint.colors.get rejects stale palette writes; every request also captures a palette generation, so concurrent/stale preparations cannot overwrite newer colors. Codes are interpreted in the current document's working ICC, or sRGB with no document, and remain numerically unchanged across profile changes/document replacement. Native color wells and eyedropper use this same command, then brush/fill/gradient read the synchronized state. Does not draw, change document samples/revision/dirty/history or project metadata; not undoable by document/AI-turn undo. Native user setters are locked while AI owns control, including pause; same-turn running AI may configure colors directly. Strict fields, no batch, one idempotent receipt. 中文：设置前景色、设置背景色、选择绘画颜色、拾色器、工具调色板。",
        json!({"foreground":[0.8,0.2,0.1,1]})
    );
    command!(
        "paint.colors.describe",
        Empty,
        Query,
        "读取颜色面板数值",
        "Describe color panel values",
        "Read this editor's foreground/background as exact normalized working-RGB RGBA, rounded8/16-bit RGBA, HSB [hue degrees,saturation percent,brightness percent], canonical #RRGGBBAA and one palette version. These are encodings of the document working RGB (sRGB without a document), not conversions into sRGB or another ICC. HSB uses encoded RGB with achromatic hue0; HEX and rgb8 round to8-bit, rgb16 rounds to16-bit, rgba retains the full palette float. Pure read, allowed without document and while AI owns control; no sample/revision/dirty/history/current-layer/preview changes. Native persistent color panel and AI share the same formatter. 中文：颜色面板、读取RGB数值、读取HSB、读取HEX、十六进制颜色、颜色透明度、前景背景色。",
        json!({})
    );
    command!(
        "paint.colors.set_components",
        SetColorComponents,
        View,
        "按RGB HSB或HEX设置颜色",
        "Set color from RGB HSB or HEX",
        "Set one explicit foreground/background palette target from tagged color.model: rgb normalized red/green/blue0..1, rgb8 integer0..255, rgb16 integer0..65535, hsb hue0..360/saturation0..100/brightness0..100, or hex value RRGGBB/RRGGBBAA with optional#. Non-hex models accept alpha0..1 default1; six-digit hex is opaque, eight-digit hex includes8-bit alpha. Reject nonfinite/out-of-range values, wrong integer types, malformed HEX, unknown fields and stale expected_version. All formats encode the current working RGB, not sRGB/ICC conversion; hue360 wraps0 and HSB is encoded RGB. HEX input is8-bit precision even in a16-bit document; full normalized RGB remains available. Native persistent color panel sends these exact parameters through this same parser/setter, then brush/fill/gradient read synchronized colors. Palette version advances once, preserves other target and current layer, no document samples/revision/dirty/history/project metadata, not document/turn undoable, no batch. Same source/session/palette/control ownership as paint.colors.set; native writes blocked during AI ownership, owning AI may write. 中文：常驻颜色面板、RGB数值设置、HSB颜色、HEX颜色、十六进制颜色、颜色透明度、前景色和背景色。",
        json!({"target":"foreground","color":{"model":"hsb","hue":210,"saturation":50,"brightness":80,"alpha":0.5}})
    );
    command!(
        "paint.colors.swap",
        Empty,
        View,
        "交换前景色与背景色",
        "Swap foreground/background colors",
        "Swap this editor's two normalized working-RGB palette colors; native X uses this command. Increments palette version, rejects stale/concurrent prepared palette calls, keeps current layer selection and all document state/history unchanged. Per-editor tool state, not a pixel edit and not document/turn undoable; no batch. Native user blocked during AI ownership, same-turn running AI allowed. 中文：交换前景色与背景色、交换绘画颜色、X 快捷键。",
        json!({})
    );
    command!(
        "paint.colors.reset",
        Empty,
        View,
        "重置前景色与背景色",
        "Reset foreground/background colors",
        "Reset this editor's palette to opaque encoded working-RGB black foreground and white background; native D uses this command. Same strict ownership, palette-version/idempotent receipt and no-document semantics as paint.colors.set; no pixel/document/history/dirty change and not document/AI-turn undoable, no batch. 中文：重置前景色与背景色、默认黑白绘画颜色、D 快捷键。",
        json!({})
    );
    command!(
        "image.trim",
        TrimCanvas,
        Mutate,
        "裁去透明边",
        "Trim transparent edges",
        "Trim selected sides of the current artboard to the finite alpha>0 bounds of its actually visible composite, including groups/clipping/masks/adjustments and editable styles. top/bottom/left/right booleans default true; choose at least one. Hidden layers/hidden ancestors do not contribute and transparent hidden RGB never counts. Check original 8/16-bit/float composite alpha directly, no 8-bit display threshold. Only scan within current artboard, at most 64M visits, one pooled composite tile at a time with cancellation; source content outside it does not expand the trim. Preserve raw pixel samples, source positions, editable layer definitions/locks and stored off-artboard content; use image.crop pixels=delete separately if deletion is intended. Rebind current/previous selections and freeze generated/style/spatial references through the same canvas path. Fully transparent composite rejects without changes. Already tight selected sides returns changed=false without history/revision/dirty/layer-selection changes; otherwise one atomic history/undo, not batchable. Native and AI use the same command. 中文：裁去透明边、去掉透明边框、自动裁切透明区域、顶部/底部/左边/右边；保留可编辑图层与画板外原码。",
        json!({})
    );
    command!(
        "image.rotate",
        RotateImage,
        Mutate,
        "整图任意角度旋转",
        "Rotate pixel image by angle",
        "Rotate the layered image clockwise by finite angle_degrees +/-360000 (normalized modulo360); interpolation nearest/bilinear(default). General angles currently support every pixel layer, unstyled editable text and unstyled group, including hidden/off-artboard stored pixels, linked/disabled u16 masks and current/previous selections. Generated fills/adjustments, active styles and frozen style references reject before work; no implicit rasterization. Exact 90/180/270 degrees reuse image.orient and its support for all current types/finite generated crops/styles. Exact multiples of360 are no-ops with no history/revision/dirty/current-layer change. For general angles, rotate around old artboard center and translate its conservative half-source-pixel reconstruction envelope to the old artboard top-left; new canvas dimensions enclose that envelope independent of interpolation, so nearest may show an extra transparent border. Pixel source outside the old artboard is never clipped; it can stay outside the new frame. Integer layer storage origins rebase to transformed source bounds. Document selections transform/rebind to the new artboard, layer/group masks keep transformed finite support and enabled flags. ICC/depth/tree/IDs/clipping remain; fractions use premultiplied linear light, nearest preserves sampled codes and picks the positive-side sample for floating edge ties within 1e-9 source pixel. Reject any layer/ancestor pixel/position/transparency/all lock or existing 64M-output, eight-tile/4096-source-decode, coordinate and shared memory limits. Prepare from immutable source, row cancellation, atomic one-history undo, not batchable, exact AI content-replacement approval. This partial image-rotation capability does not complete general editable fill/style geometry. Text retains source plus the ordered geometry pipeline; re-edit preserves orientation and crop barriers. 中文：整图任意角度旋转、旋转图像、顺时针角度、逆时针角度；扩展画板，保留像素层/组、蒙版、选区和画板外内容。",
        json!({"angle_degrees":15,"interpolation":"bilinear"})
    );
    command!(
        "image.resize",
        ResizeImage,
        Mutate,
        "图像大小与重采样",
        "Resize layered pixel image",
        "Resize the actual image to integer width/height 1..100000 pixels while keeping the old artboard top-left and document DPI. Supports all pixel layers, unstyled editable text and unstyled groups, including hidden/off-artboard source pixels, layer/group masks (enabled flag retained) and current/previous u16 selections. Preserve IDs/tree/clipping/ICC/bit depth; do not flatten or clip to the old/new artboard. Generated fills/adjustments, active styles and frozen style references explicitly reject before resampling; their editable sizing support remains open. Text retains source and ordered sizing/crop operations, replaying the same selected nearest/bilinear/area u16 glyph coverage sampler after source edits. Font size/box remain source-layout values; transformed ink bounds are queried separately. interpolation=nearest copies exact source codes (including transparent hidden RGB); bilinear samples pixel centers in premultiplied linear light with actual ICC; area(default) integrates exact pixel-box overlap on each reducing axis and uses bilinear on equal/enlarging axes. Finite interior/off-artboard source edges filter against transparent zero, with conservative half-source-pixel support for bilinear axes; only source edges exactly coinciding with the old artboard edge extend their sample so a full opaque image keeps opaque borders. Area reduction uses exact pixel-box overlap normalization. Rational canvas coordinates preserve sample phase at large offsets. Source bounds scale with floor/ceil and integer layer storage origins rebase; this can retain a partial edge pixel beyond the new artboard. Selections rebind to the exact new frame, never reapply old selection to image pixels. Equal dimensions are a no-op with no history/revision/dirty/current-layer changes even on unsupported or locked layers. Reject affected layer/ancestor pixel/position/transparency/all locks. At most 64M destination visits and 256M sample taps across layers/masks/selections per command, eight cached source tiles/4096 decoded loads per raster, shared reservations and row/inner-loop cancellation. Prepare from immutable sources, publish one atomic history step with exact undo, not batchable; exact AI pixels.replace approval. Native and AI share this implementation. 中文：图像大小、调整图像尺寸、像素尺寸、重采样、最近邻、双线性、面积缩小、抗混叠；保留图层蒙版与画板外像素。",
        json!({"width":12,"height":10,"interpolation":"area"})
    );
    command!(
        "image.straighten",
        StraightenImage,
        Mutate,
        "拉直图像与自动裁剪",
        "Straighten image and crop",
        "Straighten all pixel layers, unstyled editable text and unstyled groups in one atomic history step using reference.type=angle (clockwise angle_degrees +/-45) or line (distinct start/end canvas points, align horizontal/vertical). A reference line computes the shortest unoriented correction toward the requested axis; choose the other axis if correction exceeds45 degrees. Points must be finite inside +/-1e12 and at least1e-6 pixel apart. crop=true(default) uses the rotated original artboard, not visible-content alpha, to compute an analytical largest centered continuous inscribed rectangle, then rounds its edges inward and verifies inverse corners. Bilinear uses an additional half-source-pixel inset so an originally opaque image cannot gain transparent corner interpolation; integer alignment may conservatively remove extra edge pixels. An image too narrow for a nonempty safe integer rectangle rejects before sampling; set crop=false to retain the expanded rotation artboard. No content-aware filling, deletion or implicit rasterization. Same immutable resampler/limits/locks/partial-type restrictions as image.rotate; preserve layer IDs/tree/depth/ICC, hidden/off-artboard sources, enabled flags and layer/group/current/previous masks. Source content outside the cropped frame remains stored; selection binds to final frame. Exactly zero correction is unchanged-state no-op, even with crop=true. Native numeric and canvas-dragged reference lines use the same strict parameters/output as AI, with pixels.replace approval for AI. A native line stages only a guide, binds the original document/revision/control epoch and operation-specific ownership token, and applies on Enter or its button; Escape, tool/document/revision/control changes cancel it without revoking newer AI work. Horizontal/vertical, crop and interpolation remain adjustable before confirmation. One undo restores rotation and crop. Generated fills/adjustments/styles arbitrary geometry remains open; text retains source and ordered geometry/crop stages. 中文：拉直、水平线校正、垂直线校正、按两点参考线拉直、画布拖线拉直、参考线草稿、回车应用、Esc取消、校正地平线、自动裁掉旋转空角、保留像素裁剪。",
        json!({"reference":{"type":"angle","angle_degrees":5},"crop":true,"interpolation":"bilinear"})
    );
    command!(
        "image.convert_bit_depth",
        ConvertBitDepth,
        Mutate,
        "转换文档位深",
        "Convert document bit depth",
        "Convert the entire layered RGB document between bit_depth=8 and16 without flattening, resampling, changing its ICC or using display/composite caches. Every stored RGBA channel, including alpha-zero hidden RGB and hidden/off-artboard source, converts by exact integer rules:8->16 multiplies by257;16->8 rounds (code+128)/257, with no dithering and possible RGB/alpha precision loss. Convert solid/gradient fill and every editable style RGBA endpoint too; groups, adjustment definitions and all layer IDs/tree/clipping/positions/crop/style/spatial references/DPI stay editable. Normalized floating adjustment colors stay normalized. Current/previous selection and layer/group mask u16 samples, layouts, anchors, enabled flags and tile versions remain unchanged. Whole-document operation independent of active selection/layer. Pixel/transparency/all locks on any layer/ancestor reject; position-only locks are allowed. The same depth is a no-op before lock/work checks, preserving revision/history/dirty/current layer. Validate target frame, frozen references, total pixel+mask bytes and tile capacity before conversion; bounded tile/row cancellation and pooled scratch/output with original source snapshots. One atomic history with exact undo and project roundtrip, not batchable, pixels.replace approval for AI. Native menu/panel and AI execute this same command. 中文：文档位深、8位转16位、16位转8位、转换文档位深、提高位深、降低位深、保留图层和蒙版；降低位深会量化颜色与透明度。",
        json!({"bit_depth":8})
    );
    for item in &mut list {
        item.returns = match item.id {
            "view.guides.list" => schemars::schema_for!(GuidesResult),
            "view.guides.add" | "view.guides.update" | "view.guides.delete" => {
                schemars::schema_for!(GuideResult)
            }
            "view.guides.clear" => schemars::schema_for!(ClearGuidesResult),
            "view.presentation.get" | "view.presentation.set" => {
                schemars::schema_for!(crate::Presentation)
            }
            "layer.snap_position" => schemars::schema_for!(SnapPositionResult),
            "type.fonts" => schemars::schema_for!(FontsResult),
            "type.create" | "type.update" | "type.inspect" => schemars::schema_for!(TextResult),
            "image.auto" => schemars::schema_for!(AutoCorrectionResult),
            "image.rotate" => schemars::schema_for!(RotateImageResult),
            "image.resize" => schemars::schema_for!(ResizeImageResult),
            "image.straighten" => schemars::schema_for!(StraightenImageResult),
            "image.convert_bit_depth" => schemars::schema_for!(BitDepthResult),
            "image.trim" => schemars::schema_for!(TrimCanvasResult),
            "paint.colors.describe" => schemars::schema_for!(PaintColorDescription),
            "paint.colors.get"
            | "paint.colors.set"
            | "paint.colors.set_components"
            | "paint.colors.swap"
            | "paint.colors.reset" => schemars::schema_for!(PaintColors),
            "image.orient" => schemars::schema_for!(OrientImageResult),
            "image.canvas_size" | "image.crop" | "image.reveal_all" => {
                schemars::schema_for!(CanvasGeometryResult)
            }
            "layer.pixel_bounds" => schemars::schema_for!(LayerBoundsResult),
            "transform.apply" => schemars::schema_for!(TransformResult),
            "retouch.spot_heal" => schemars::schema_for!(SpotHealResult),
            "retouch.heal" => schemars::schema_for!(HealResult),
            "paint.sample_color" => schemars::schema_for!(SampleColorResult),
            "paint.fill"
            | "paint.bucket"
            | "paint.gradient"
            | "filter.gaussian_blur"
            | "filter.high_pass"
            | "filter.unsharp_mask"
            | "filter.add_noise" => schemars::schema_for!(BrushResult),
            "retouch.dodge" | "retouch.burn" | "retouch.sponge" => {
                schemars::schema_for!(BrushResult)
            }
            "paint.stroke" | "paint.erase" | "retouch.clone" => schemars::schema_for!(BrushResult),
            "document.inspect" => schemars::schema_for!(Snapshot),
            "layer.list" => schemars::schema_for!(LayerListResult),
            "edit.copy" | "edit.copy_merged" => schemars::schema_for!(ClipboardCopyResult),
            "edit.clipboard.write_status" => schemars::schema_for!(ClipboardWriteResult),
            "layer.align" | "layer.distribute" => schemars::schema_for!(MultiLayerResult),
            "document.histogram" => schemars::schema_for!(HistogramResult),
            "select.subject" => schemars::schema_for!(SubjectResult),
            "select.ellipse" | "select.polygon" | "select.lasso" | "select.reselect"
            | "select.feather" | "select.expand" | "select.contract" | "select.smooth"
            | "select.border" | "select.transform" | "select.wand" | "select.quick" => {
                schemars::schema_for!(SelectionResult)
            }
            "select.marquee" | "select.all" | "select.none" | "select.inverse" => {
                schemars::schema_for!(SelectionResult)
            }
            "file.recent.list" => schemars::schema_for!(RecentListResult),
            "file.new" | "file.open" | "file.recover" => schemars::schema_for!(NewResult),
            "file.save" | "file.save_as" | "file.export" => schemars::schema_for!(ExternalResult),
            "edit.undo" | "edit.redo" | "edit.undo_turn" => schemars::schema_for!(HistoryResult),
            "view.pan" | "view.zoom" | "view.fit" | "view.actual_size" | "layer.select" => {
                schemars::schema_for!(ViewResult)
            }
            "document.batch" => schemars::schema_for!(BatchResult),
            _ => schemars::schema_for!(LayerResult),
        };
        if matches!(item.id, "file.open" | "file.place" | "file.recover") {
            item.effects.push("file.read");
            item.permissions.push("read_resource");
        }
        if item.kind == CommandKind::External {
            item.effects = vec!["document.read", "file.write"];
            item.permissions.push("write_resource");
        }
        if matches!(item.id, "edit.copy" | "edit.copy_merged") {
            item.effects = vec!["document.read", "clipboard.write"];
            item.permissions = vec!["bound_document", "clipboard.write_confirmation"];
            item.batchable = false;
        }
        if matches!(
            item.id,
            "edit.cut" | "edit.paste" | "edit.paste_in_place" | "edit.clipboard.write_status"
        ) {
            item.batchable = false;
            item.permissions.push("clipboard_resource");
            item.effects.push("clipboard.read");
        }
        if item.id == "file.export" {
            item.examples.extend([
                json!({"resource":"fixture-export","format":"webp","webp_lossless":false,"quality":80,"allow_quantization":true}),
                json!({"resource":"fixture-export","format":"png","width":8,"height":4}),
                json!({"resource":"fixture-export","format":"tiff","target_space":"display_p3"}),
                json!({"resource":"fixture-export","format":"jpeg","quality":90,"background":[255,255,255],"target_space":"srgb","allow_quantization":true}),
                json!({"resource":"fixture-export","format":"webp","allow_quantization":true}),
            ]);
        }
        if item.id == "file.new" {
            item.examples.extend([
            json!({"preset":"a4_300dpi","bit_depth":16,"background":[65535,65535,65535,65535],"working_space":"display_p3"}),
            json!({"width":128,"height":96,"bit_depth":8,"resolution_dpi":150,"background":[10,20,30,128]})]);
        }
        if item.id == "layer.add_tonal_adjustment" {
            item.examples.extend([
                json!({"name":"Brightness","adjustment":{"type":"brightness_contrast","brightness":10,"contrast":5}}),
                json!({"name":"Curves","adjustment":{"type":"curves","master":[[0,0],[0.5,0.65],[1,1]]}}),
            ]);
        }
        if item.id == "adjust.tonal.set" {
            item.examples.extend([
                json!({"layer":{"name":"Brightness"},"adjustment":{"type":"brightness_contrast","brightness":-5,"contrast":10}}),
                json!({"layer":{"name":"Curves"},"adjustment":{"type":"curves","master":[[0,0],[0.3,0.4],[0.7,0.8],[1,1]]}}),
            ]);
        }
        if matches!(item.id, "layer.add_tonal_adjustment" | "adjust.tonal.set") {
            for settings in [
                json!({"type":"vibrance","vibrance":25,"saturation":5}),
                json!({"type":"white_balance","temperature":20,"tint":-10,"preserve_luminance":true}),
                json!({"type":"shadows_highlights","shadows":35,"highlights":25,"shadows_width":0.5,"highlights_width":0.5,"radius":30}),
                json!({"type":"color_balance","midtones":[15,0,-10]}),
                json!({"type":"black_white","weights":[40,60,40,60,20,80],"tint":[0.8,0.6,0.4],"tint_amount":20}),
                json!({"type":"photo_filter","color":[1,0.65,0.3],"density":25}),
                json!({"type":"channel_mixer","matrix":[[0,0,1,0],[0,1,0,0],[1,0,0,0]]}),
                json!({"type":"invert"}),
                json!({"type":"posterize","levels":6}),
                json!({"type":"threshold","level":0.45}),
                json!({"type":"gradient_map","stops":[{"at":0,"color":[0,0,0.2]},{"at":1,"color":[1,0.9,0.5]}]}),
                json!({"type":"selective_color","ranges":[[10,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0]]}),
                json!({"type":"desaturate"}),
            ] {
                let name = settings["type"].as_str().expect("static type");
                item.examples
                    .push(if item.id == "layer.add_tonal_adjustment" {
                        json!({"name":name,"adjustment":settings})
                    } else {
                        json!({"layer":{"name":name},"adjustment":settings})
                    });
            }
        }
        if item.id == "document.batch"
            || item.id == "file.place"
            || item.id.starts_with("filter.")
            || item.id.starts_with("type.")
            || matches!(
                item.id,
                "transform.apply"
                    | "layer.pixel_bounds"
                    | "image.canvas_size"
                    | "image.crop"
                    | "image.reveal_all"
            )
        {
            item.batchable = false;
        }
        if item.id.starts_with("paint.colors.") {
            item.batchable = false;
            item.permissions = vec!["bound_editor", "palette_version", "control_ownership"];
            item.effects = if item.kind == CommandKind::Query {
                vec!["tool.colors.read"]
            } else {
                vec!["tool.colors.write"]
            };
        }
        if item.id == "type.fonts" {
            item.effects = vec!["font.catalog.read"];
            item.permissions = vec!["bound_editor"];
        }
        if item.id.starts_with("view.guides.") || item.id == "layer.snap_position" {
            item.batchable = false;
        }
        if item.id.starts_with("view.presentation.") {
            item.batchable = false;
            item.permissions = vec!["bound_editor", "presentation_version"];
            item.effects = if item.kind == CommandKind::Query {
                vec!["view.presentation.read"]
            } else {
                vec!["view.presentation.write"]
            };
        }
        if item.id == "type.create" {
            item.examples.push(json!({"name":"Paragraph","spec":{"text":"中文段落与Latin words\n保留文字源和换行","font":"cjk","font_size":18,"line_height":1.3,"tracking":0,"align":"right","layout":{"type":"paragraph","width":120,"height":80}},"color":[0.3,0.5,0.7,1],"x":1,"y":1}));
        }
        if item.id == "paint.colors.set_components" {
            for color in [
                json!({"model":"rgb","red":0.1,"green":0.2,"blue":0.3}),
                json!({"model":"rgb8","red":51,"green":102,"blue":153,"alpha":0.5}),
                json!({"model":"rgb16","red":10001,"green":30003,"blue":50001}),
                json!({"model":"hex","value":"#33669980"}),
            ] {
                item.examples
                    .push(json!({"target":"background","color":color}));
            }
        }
        if item.id == "image.trim" {
            item.batchable = false;
        }
        if item.id == "transform.apply" {
            item.examples.push(json!({"layer":{"name":"GroupGeometry"},"scope":"whole_layer","geometry":{"type":"components","scale":[0.8,1.2],"rotation_degrees":10,"pivot":[8,8]},"interpolation":"bilinear"}));
            item.examples.push(json!({"layer":{"name":"TypeGeometry"},"scope":"whole_layer","geometry":{"type":"components","scale":[1.2,0.8],"rotation_degrees":15,"pivot":[8,8]},"interpolation":"bilinear"}));
            item.examples.push(json!({"layer":{"name":"Background"},"scope":"selection","geometry":{"type":"components","translation":[4,2]}}));
            item.examples.push(json!({"layer":{"name":"Background"},"scope":"whole_layer","geometry":{"type":"corners","source":[[0,0],[16,0],[16,16],[0,16]],"destination":[[2,1],[14,2],[16,14],[0,15]]},"interpolation":"bilinear"}));
        }
        if item.id == "image.crop" {
            item.effects.push("pixels.delete_if_requested");
            item.examples
                .push(json!({"rect":{"x":2,"y":3,"width":12,"height":9},"pixels":"delete"}));
        }
        if item.id == "image.orient" {
            item.batchable = false;
            for operation in [
                "rotate_90_ccw",
                "rotate_180",
                "flip_horizontal",
                "flip_vertical",
            ] {
                item.examples.push(json!({"operation":operation}));
            }
        }
        if item.id == "image.resize" {
            for interpolation in ["nearest", "bilinear"] {
                item.examples
                    .push(json!({"width":23,"height":19,"interpolation":interpolation}));
            }
        }
        if item.id == "image.auto" {
            item.examples.push(json!({"layer":{"name":"Background"},"settings":{"method":"contrast","clip_percent":0}}));
            item.examples.push(json!({"layer":{"name":"Background"},"settings":{"method":"color","clip_percent":1,"neutralize_strength":0.7}}));
            item.batchable = false;
            item.effects.push("pixels.replace");
        }
        if item.id == "filter.high_pass" {
            item.effects.push("pixels.replace");
            item.examples
                .push(json!({"layer":{"name":"Background"},"sigma":2,"space":"linear_p3"}));
        }
        if item.id == "image.straighten" {
            item.examples.push(json!({"reference":{"type":"line","start":[2,4],"end":[13,5],"align":"horizontal"},"crop":true,"interpolation":"nearest"}));
            item.examples.push(json!({"reference":{"type":"line","start":[4,2],"end":[5,13],"align":"vertical"},"crop":false}));
        }
        if item.id == "image.convert_bit_depth" {
            item.examples.push(json!({"bit_depth":16}));
        }
        if matches!(
            item.id,
            "image.rotate" | "image.resize" | "image.straighten" | "image.convert_bit_depth"
        ) {
            item.batchable = false;
        }
        if matches!(
            item.id,
            "transform.apply"
                | "image.orient"
                | "image.rotate"
                | "image.resize"
                | "image.straighten"
                | "image.convert_bit_depth"
        ) {
            item.effects.push("pixels.replace");
        }
        if matches!(
            item.id,
            "retouch.clone"
                | "retouch.heal"
                | "retouch.spot_heal"
                | "retouch.dodge"
                | "retouch.burn"
                | "retouch.sponge"
        ) {
            item.batchable = false;
        }
        if matches!(item.id, "paint.stroke" | "paint.erase") {
            item.batchable = false;
            item.examples.push(json!({"layer":{"name":"Background"},"target":"mask","points":[{"x":8.5,"y":8.5,"pressure":0.5}],"size":4,"value":if item.id=="paint.erase"{1.0}else{0.25},"opacity":0.5,"pressure_size":false}));
        }
        if matches!(
            item.id,
            "paint.sample_color" | "paint.fill" | "paint.bucket" | "paint.gradient"
        ) {
            item.batchable = false;
        }
        if item.id == "paint.gradient" {
            for shape in ["radial", "angle", "reflected", "diamond"] {
                item.examples.push(json!({"layer":{"name":"Background"},"start":[8.5,8.5],"end":[14.5,8.5],"stops":[{"at":0,"color":[0,0,0,1]},{"at":1,"color":[1,1,1,1]}],"shape":shape}));
            }
        }
        if item.id == "select.subject" {
            item.batchable = false;
            item.permissions.push("verified_local_subject_model");
        }
        if matches!(
            item.id,
            "select.ellipse"
                | "select.polygon"
                | "select.lasso"
                | "select.feather"
                | "select.expand"
                | "select.contract"
                | "select.smooth"
                | "select.border"
                | "select.transform"
                | "select.wand"
                | "select.quick"
        ) {
            item.batchable = false;
        }
        if item.id == "file.recent.list" {
            item.batchable = false;
            item.effects = vec!["history.read", "file.metadata.read"];
            item.permissions = vec!["host_recent_visibility"];
        }
    }
    command!(
        "color.assign",
        AssignProfile,
        Mutate,
        "指定文档配置文件",
        "Assign document profile",
        "Assign srgb or display_p3 interpretation without converting any integer pixel samples, hidden RGB, generated fill/style color codes, masks, depth, geometry or original source ICC. Appearance changes because identical RGB codes are interpreted in the newly assigned working profile. This is distinct from color.convert, which changes samples to preserve appearance. Pixel/transparency/position locks are allowed because source codes are unchanged; all-locked layers reject document reinterpretation. Undo restores exact old ICC/metadata; project save/reopen keeps the assigned profile. Cancellable, budgeted, one atomic history edit, not batchable. Currently supports the two built-in RGB working presets, not arbitrary ICC file assignment or CMYK. 中文：指定配置文件、保留 RGB 数值、重新解释颜色、不要转换像素。",
        json!({"space":"display_p3"})
    );
    if let Some(last) = list.last_mut() {
        last.returns = schemars::schema_for!(ColorResult);
        last.batchable = false;
    }
    // Conversion changes source codes to preserve appearance. Keep the
    // assignment/conversion choice explicit in the catalog and native UI.
    command!(
        "color.convert",
        ConvertColor,
        Mutate,
        "转换工作空间",
        "Convert working space",
        "Convert source tiles and generated fill/style colors at their original bit depth to srgb or display_p3, preserving appearance and exact alpha. Pixel/all locks reject conversion; masks stay unchanged. Unlike color.assign this changes RGB codes. Per-layer/tile cancellation and shared scratch budgets; one undoable edit restores exact original samples and ICC. Not batchable. 中文：转换配置文件、转换工作空间、保持颜色。",
        json!({"space":"display_p3"})
    );
    if let Some(last) = list.last_mut() {
        last.returns = schemars::schema_for!(ColorResult);
        last.batchable = false;
    }
    list
}

pub(crate) fn kind(id: &str) -> Result<CommandKind, CommandError> {
    catalog()
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.kind)
        .ok_or_else(|| CommandError::Invalid(format!("unknown command {id}")))
}
