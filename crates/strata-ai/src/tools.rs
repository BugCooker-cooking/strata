use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

macro_rules! arguments {
    ($name:ident { $($(#[$a:meta])* $field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name { $( $(#[$a])* pub $field: $ty ),* }
    }
}
arguments!(Browse { #[serde(default)] path: Option<String> });
arguments!(Search {
    query: String,
    #[serde(default = "default_limit")]
    limit: usize
});
fn default_limit() -> usize {
    12
}
arguments!(Describe { ids: Vec<String> });
arguments!(Invoke {
    id: String,
    params: Value
});
arguments!(Batch { steps: Vec<strata_core::commands::BatchStep> });
arguments!(Inspect {});
arguments!(Ask { question: String, #[serde(default)] options: Vec<String> });

#[derive(Clone, Debug, Deserialize, serde::Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub x: i64,
    pub y: i64,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Deserialize, serde::Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CanvasRequest {
    #[serde(default)]
    pub region: Option<Region>,
    #[serde(default)]
    pub layers: Vec<String>,
    #[serde(default = "default_size")]
    pub max_size: u32,
}
fn default_size() -> u32 {
    1024
}

/// Registry-independent, transport-neutral schemas. These are the complete
/// model-visible tool list; command descriptions are discovered lazily.
pub fn definitions() -> Vec<Value> {
    let mut tools = Vec::new();
    macro_rules! tool {
        ($id:literal,$params:ty,$description:literal) => {
            tools.push(json!({"name":$id,"description":$description,"parameters":schemars::schema_for!($params)}));
        }
    }
    tool!(
        "browse_catalog",
        Browse,
        "Browse one level of the implemented command catalog. Empty path lists domains."
    );
    tool!(
        "search_catalog",
        Search,
        "Search command IDs, English and Chinese descriptions; limit 1..32."
    );
    tool!(
        "describe_commands",
        Describe,
        "Read complete schemas, permissions and examples for 1..8 command IDs."
    );
    tool!(
        "invoke_command",
        Invoke,
        "Execute one registered command on the host-bound document; permissions are enforced by the host."
    );
    tool!(
        "invoke_batch",
        Batch,
        "Atomically execute same-document query/mutate steps; external/lifecycle/history commands are rejected. Supports prior result references."
    );
    tool!(
        "inspect_document",
        Inspect,
        "Read authorized document metadata, visible layers, current selection and recent history. No arbitrary paths or other documents."
    );
    tool!(
        "view_canvas",
        CanvasRequest,
        "Read an authorized visible composite/region, max_size 32..2048. Returns statistics if vision is unavailable. Hidden layers require new host authorization."
    );
    tool!(
        "ask_user",
        Ask,
        "Ask a question and pause for the user's answer. Optional 2..6 suggested options; free text is always accepted."
    );
    tools
}
