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
        "Create one artboard and a transparent pixel layer; replacing dirty content requires discard_changes.",
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
        "layer.list",
        Empty,
        Query,
        "图层列表",
        "List layers",
        "Read layers in bottom-to-top order. Stable IDs are preferred over names.",
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
        "Export PNG or TIFF from document samples/composition at the original bit depth, embedding the working ICC.",
        json!({"resource":"fixture-export","format":"png"})
    );
    for item in &mut list {
        item.returns = match item.id {
            "document.inspect" => schemars::schema_for!(Snapshot),
            "layer.list" => schemars::schema_for!(LayerListResult),
            "file.new" | "file.open" => schemars::schema_for!(NewResult),
            "file.save" | "file.save_as" | "file.export" => schemars::schema_for!(ExternalResult),
            "edit.undo" | "edit.redo" | "edit.undo_turn" => schemars::schema_for!(HistoryResult),
            "view.pan" | "view.zoom" | "view.fit" | "layer.select" => {
                schemars::schema_for!(ViewResult)
            }
            "document.batch" => schemars::schema_for!(BatchResult),
            _ => schemars::schema_for!(LayerResult),
        };
        if item.id == "file.open" {
            item.effects.push("file.read");
            item.permissions.push("read_resource");
        }
        if item.kind == CommandKind::External {
            item.effects = vec!["document.read", "file.write"];
            item.permissions.push("write_resource");
        }
        if item.id == "document.batch" {
            item.batchable = false;
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
