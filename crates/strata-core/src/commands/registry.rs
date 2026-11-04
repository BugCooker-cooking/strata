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
        ($id:literal,$params:ty,$kind:ident,$zh:literal,$en:literal,$description:literal,$example:expr) => {{
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
        "Choose normal, multiply, screen or overlay. Document samples remain unchanged.",
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
    for item in &mut list {
        item.returns = match item.id {
            "document.inspect" => schemars::schema_for!(Snapshot),
            "layer.list" => schemars::schema_for!(LayerListResult),
            "document.histogram" => schemars::schema_for!(HistogramResult),
            "select.subject" => schemars::schema_for!(SubjectResult),
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
        if item.id == "document.batch" || item.id == "file.place" {
            item.batchable = false;
        }
        if item.id == "select.subject" {
            item.batchable = false;
            item.permissions.push("verified_local_subject_model");
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
