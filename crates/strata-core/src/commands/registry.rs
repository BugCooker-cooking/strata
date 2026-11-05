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
        ($id:literal,$params:ty,$kind:ident,$zh:literal,$en:literal,$description:expr,$example:expr) => {{
            let kind=CommandKind::$kind;
            list.push(CommandSpec { id:$id,path:$id.split('.').collect(),title_zh:$zh,title_en:$en,summary:$en,description:$description,kind,
                effects:match kind { CommandKind::Query=>vec!["document.read"], CommandKind::View=>vec!["view.write"],_=>vec!["document.write"] },
                permissions:vec!["bound_document"],batchable:matches!(kind,CommandKind::Query|CommandKind::Mutate),undoable:kind==CommandKind::Mutate,
                params:schemars::schema_for!($params),returns:schemars::schema_for!(Value),
                preconditions:vec!["Host-bound source, session, document and revision; control ownership and resource limits are checked."],examples:vec![$example] });
        }}
    }
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
        "Multiply a pixel layer's original alpha by its enabled u16 mask without rewriting hidden RGB. Locked pixels/transparency are protected; fills/groups are rasterized with the mask baked. Adjustments require explicit bake_backdrop=true.",
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
    for item in &mut list {
        item.returns = match item.id {
            "paint.stroke" | "paint.erase" => schemars::schema_for!(BrushResult),
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
            "view.pan" | "view.zoom" | "view.fit" | "layer.select" => {
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
        if item.id == "document.batch" || item.id == "file.place" {
            item.batchable = false;
        }
        if matches!(item.id, "paint.stroke" | "paint.erase") {
            item.batchable = false;
            item.examples.push(json!({"layer":{"name":"Background"},"target":"mask","points":[{"x":8.5,"y":8.5,"pressure":0.5}],"size":4,"value":if item.id=="paint.erase"{1.0}else{0.25},"opacity":0.5,"pressure_size":false}));
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
    // This command changes source working samples and is undoable, unlike a
    // display profile change. Keep it explicit in the catalog and native UI.
    command!(
        "color.convert",
        ConvertColor,
        Mutate,
        "转换工作空间",
        "Convert working space",
        "Convert source tiles at their original bit depth to srgb or display_p3. Alpha is preserved; undo restores exact original samples and ICC.",
        json!({"space":"display_p3"})
    );
    if let Some(last) = list.last_mut() {
        last.returns = schemars::schema_for!(ColorResult);
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
