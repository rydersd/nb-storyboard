use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub type ExtraFields = Map<String, Value>;

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_one(value: &f64) -> bool {
    *value == 1.0
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Project {
    pub schema: String,
    pub title: String,
    pub fps: u32,
    pub format: Format,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenes: Vec<Scene>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cuts: Vec<Cut>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_cut: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stickies: Vec<Sticky>,
    pub layout: Layout,
    #[serde(flatten, default)]
    pub extra: ExtraFields,
}

impl Project {
    /// Produces the canonical project document: indented JSON while keeping compact numeric
    /// arrays (notably stroke points) readable on one line.
    pub fn to_pretty_json(&self) -> serde_json::Result<String> {
        let document = serde_json::to_value(self)?;
        let mut output = String::new();
        write_pretty_value(&mut output, &document, 0, JsonContext::Project)?;
        Ok(output)
    }
}

#[derive(Clone, Copy)]
enum JsonContext {
    Normal,
    Project,
    Scenes,
    Scene,
    Shots,
    Shot,
    Takes,
    Take,
    Panels,
    Panel,
    Strokes,
    Stroke,
    CompactArray,
}

fn write_pretty_value(
    output: &mut String,
    value: &Value,
    indent: usize,
    context: JsonContext,
) -> serde_json::Result<()> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => output.push_str(&value.to_string()),
        Value::String(value) => output.push_str(&serde_json::to_string(value)?),
        Value::Array(values) => write_pretty_array(output, values, indent, context)?,
        Value::Object(values) => write_pretty_object(output, values, indent, context)?,
    }
    Ok(())
}

fn write_pretty_array(
    output: &mut String,
    values: &[Value],
    indent: usize,
    context: JsonContext,
) -> serde_json::Result<()> {
    let compact = matches!(context, JsonContext::CompactArray);
    output.push('[');
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        if !compact {
            output.push('\n');
            output.push_str(&"  ".repeat(indent + 1));
        }
        write_pretty_value(output, value, indent + 1, array_element_context(context))?;
    }
    if !values.is_empty() && !compact {
        output.push('\n');
        output.push_str(&"  ".repeat(indent));
    }
    output.push(']');
    Ok(())
}

fn write_pretty_object(
    output: &mut String,
    values: &Map<String, Value>,
    indent: usize,
    context: JsonContext,
) -> serde_json::Result<()> {
    output.push('{');
    for (index, (key, value)) in values.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push('\n');
        output.push_str(&"  ".repeat(indent + 1));
        output.push_str(&serde_json::to_string(key)?);
        output.push_str(": ");
        write_pretty_value(
            output,
            value,
            indent + 1,
            object_value_context(context, key),
        )?;
    }
    if !values.is_empty() {
        output.push('\n');
        output.push_str(&"  ".repeat(indent));
    }
    output.push('}');
    Ok(())
}

fn array_element_context(context: JsonContext) -> JsonContext {
    match context {
        JsonContext::Scenes => JsonContext::Scene,
        JsonContext::Shots => JsonContext::Shot,
        JsonContext::Takes => JsonContext::Take,
        JsonContext::Panels => JsonContext::Panel,
        JsonContext::Strokes => JsonContext::Stroke,
        JsonContext::CompactArray => JsonContext::CompactArray,
        _ => JsonContext::Normal,
    }
}

fn object_value_context(context: JsonContext, key: &str) -> JsonContext {
    match (context, key) {
        (JsonContext::Project, "scenes") => JsonContext::Scenes,
        (JsonContext::Scene, "shots") => JsonContext::Shots,
        (JsonContext::Shot, "takes") => JsonContext::Takes,
        (JsonContext::Take, "panels") => JsonContext::Panels,
        (JsonContext::Panel, "strokes") => JsonContext::Strokes,
        (JsonContext::Stroke, "points") => JsonContext::CompactArray,
        _ => JsonContext::Normal,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Format {
    pub width: u32,
    pub height: u32,
    #[serde(flatten, default)]
    pub extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Scene {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shots: Vec<Shot>,
    #[serde(flatten, default)]
    pub extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Shot {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub action: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub dialogue: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub speaker: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub camera: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    pub duration: f64,
    #[serde(default = "default_status", skip_serializing_if = "is_draft")]
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub selected_take: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub takes: Vec<Take>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
    #[serde(flatten, default)]
    pub extra: ExtraFields,
}

fn default_status() -> String {
    "draft".into()
}

fn is_draft(value: &String) -> bool {
    value == "draft"
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Take {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub panels: Vec<Panel>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
    #[serde(flatten, default)]
    pub extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Panel {
    pub id: String,
    pub time: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub direction: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub background: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Layer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strokes: Vec<Stroke>,
    #[serde(flatten, default)]
    pub extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Layer {
    pub id: String,
    #[serde(rename = "type")]
    pub layer_type: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub opacity: f64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<Crop>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(flatten, default)]
    pub extra: ExtraFields,
}

fn one() -> f64 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Crop {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(flatten, default)]
    pub extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Stroke {
    pub color: String,
    pub width: f64,
    pub points: Vec<[f64; 2]>,
    #[serde(flatten, default)]
    pub extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Cut {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<Clip>,
    #[serde(flatten, default)]
    pub extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Clip {
    pub id: String,
    pub source: ClipSource,
    #[serde(rename = "in", default, skip_serializing_if = "is_zero")]
    pub in_frame: u64,
    #[serde(rename = "out", default, skip_serializing_if = "Option::is_none")]
    pub out_frame: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u64>,
    #[serde(flatten, default)]
    pub extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClipSource {
    Shot(ShotSource),
    Cut(CutSource),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ShotSource {
    pub shot: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub take: Option<String>,
    #[serde(flatten, default)]
    pub extra: ExtraFields,
    #[serde(skip)]
    pub source_extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CutSource {
    pub cut: String,
    #[serde(flatten, default)]
    pub extra: ExtraFields,
    #[serde(skip)]
    pub source_extra: ExtraFields,
}

impl Serialize for ClipSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Shot(source) => {
                let mut map = serializer.serialize_map(Some(1 + source.source_extra.len()))?;
                map.serialize_entry("shot", source)?;
                for (key, value) in &source.source_extra {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
            Self::Cut(source) => {
                let mut map = serializer.serialize_map(Some(1 + source.source_extra.len()))?;
                map.serialize_entry("cut", source)?;
                for (key, value) in &source.source_extra {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for ClipSource {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut map = ExtraFields::deserialize(deserializer)?;
        match (map.shift_remove("shot"), map.shift_remove("cut")) {
            (Some(value), None) => Ok(Self::Shot(
                ShotSource::from_wire_value(value, map).map_err(D::Error::custom)?,
            )),
            (None, Some(value)) => Ok(Self::Cut(
                CutSource::from_wire_value(value, map).map_err(D::Error::custom)?,
            )),
            (Some(_), Some(_)) => Err(D::Error::custom(
                "clip source must contain exactly one of shot or cut",
            )),
            (None, None) => Err(D::Error::custom("clip source must contain shot or cut")),
        }
    }
}

impl ShotSource {
    fn from_wire_value(value: Value, source_extra: ExtraFields) -> Result<Self, String> {
        let mut fields = object_fields(value, "shot source")?;
        Ok(Self {
            shot: required_string(&mut fields, "shot", "shot source")?,
            take: optional_string(&mut fields, "take", "shot source")?,
            extra: fields,
            source_extra,
        })
    }
}

impl CutSource {
    fn from_wire_value(value: Value, source_extra: ExtraFields) -> Result<Self, String> {
        let mut fields = object_fields(value, "cut source")?;
        Ok(Self {
            cut: required_string(&mut fields, "cut", "cut source")?,
            extra: fields,
            source_extra,
        })
    }
}

fn object_fields(value: Value, kind: &str) -> Result<ExtraFields, String> {
    value
        .as_object()
        .cloned()
        .ok_or_else(|| format!("{kind} must be an object"))
}

fn required_string(fields: &mut ExtraFields, key: &str, kind: &str) -> Result<String, String> {
    fields
        .shift_remove(key)
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .ok_or_else(|| format!("{kind} requires a string {key}"))
}

fn optional_string(
    fields: &mut ExtraFields,
    key: &str,
    kind: &str,
) -> Result<Option<String>, String> {
    match fields.shift_remove(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(|value| Some(value.to_owned()))
            .ok_or_else(|| format!("{kind} {key} must be a string or null")),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Sticky {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default = "default_sticky_kind")]
    pub kind: String,
    pub author: Author,
    pub pin: Pin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub resolved: bool,
    pub created: String,
    #[serde(flatten, default)]
    pub extra: ExtraFields,
}

fn default_sticky_kind() -> String {
    "note".into()
}

#[derive(Clone, Debug, PartialEq)]
pub enum Author {
    Human,
    Agent {
        agent: AgentAuthor,
        extra: ExtraFields,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AgentAuthor {
    pub name: String,
    #[serde(flatten, default)]
    pub extra: ExtraFields,
}

impl Serialize for Author {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Human => serializer.serialize_str("human"),
            Self::Agent { agent, extra } => {
                let mut map = serializer.serialize_map(Some(1 + extra.len()))?;
                map.serialize_entry("agent", agent)?;
                for (key, value) in extra {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Author {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if value == Value::String("human".into()) {
            return Ok(Self::Human);
        }
        let mut map = object_fields(value, "author").map_err(D::Error::custom)?;
        let agent = map
            .shift_remove("agent")
            .ok_or_else(|| D::Error::custom("author must be human or an agent object"))?;
        let mut agent_fields = object_fields(agent, "agent author").map_err(D::Error::custom)?;
        let name =
            required_string(&mut agent_fields, "name", "agent author").map_err(D::Error::custom)?;
        Ok(Self::Agent {
            agent: AgentAuthor {
                name,
                extra: agent_fields,
            },
            extra: map,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pin {
    None,
    Scene(String),
    Shot(String),
    Take {
        shot: String,
        take: String,
    },
    Panel {
        shot: String,
        take: String,
        panel: String,
    },
    Layer {
        shot: String,
        take: String,
        panel: String,
        layer: String,
    },
    Clip {
        cut: String,
        clip: String,
    },
}

impl Pin {
    pub fn parse(value: &str) -> Result<Self, String> {
        if value == "none" {
            return Ok(Self::None);
        }
        let (kind, ids) = value
            .split_once(':')
            .ok_or_else(|| "pin must be none or a typed reference".to_owned())?;
        let ids: Vec<_> = ids.split('/').map(str::to_owned).collect();
        match (kind, ids.as_slice()) {
            ("scene", [scene]) => Ok(Self::Scene(scene.clone())),
            ("shot", [shot]) => Ok(Self::Shot(shot.clone())),
            ("take", [shot, take]) => Ok(Self::Take {
                shot: shot.clone(),
                take: take.clone(),
            }),
            ("panel", [shot, take, panel]) => Ok(Self::Panel {
                shot: shot.clone(),
                take: take.clone(),
                panel: panel.clone(),
            }),
            ("layer", [shot, take, panel, layer]) => Ok(Self::Layer {
                shot: shot.clone(),
                take: take.clone(),
                panel: panel.clone(),
                layer: layer.clone(),
            }),
            ("clip", [cut, clip]) => Ok(Self::Clip {
                cut: cut.clone(),
                clip: clip.clone(),
            }),
            _ => Err("invalid pin text form".to_owned()),
        }
    }

    pub fn as_text(&self) -> String {
        match self {
            Self::None => "none".to_owned(),
            Self::Scene(scene) => format!("scene:{scene}"),
            Self::Shot(shot) => format!("shot:{shot}"),
            Self::Take { shot, take } => format!("take:{shot}/{take}"),
            Self::Panel { shot, take, panel } => format!("panel:{shot}/{take}/{panel}"),
            Self::Layer {
                shot,
                take,
                panel,
                layer,
            } => format!("layer:{shot}/{take}/{panel}/{layer}"),
            Self::Clip { cut, clip } => format!("clip:{cut}/{clip}"),
        }
    }
}

impl Serialize for Pin {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_text())
    }
}

impl<'de> Deserialize<'de> for Pin {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

#[cfg(feature = "schema")]
fn wire_schema(value: Value) -> schemars::Schema {
    serde_json::from_value(value).expect("hand-written wire schema is valid")
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for ClipSource {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ClipSource".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        wire_schema(serde_json::json!({
            "oneOf": [
                {"type": "object", "required": ["shot"], "not": {"required": ["cut"]}, "properties": {"shot": {"type": "object", "required": ["shot"], "properties": {"shot": {"type": "string"}, "take": {"type": ["string", "null"]}}}}},
                {"type": "object", "required": ["cut"], "not": {"required": ["shot"]}, "properties": {"cut": {"type": "object", "required": ["cut"], "properties": {"cut": {"type": "string"}}}}}
            ]
        }))
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for Author {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Author".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        wire_schema(serde_json::json!({
            "oneOf": [
                {"type": "string", "const": "human"},
                {"type": "object", "required": ["agent"], "properties": {"agent": {"type": "object", "required": ["name"], "properties": {"name": {"type": "string"}}}}}
            ]
        }))
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for Pin {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Pin".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        wire_schema(serde_json::json!({
            "type": "string",
            "pattern": "^(none|scene:[A-Za-z0-9_.-]+|shot:[A-Za-z0-9_.-]+|take:[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+|panel:[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+|layer:[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+|clip:[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)$"
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Layout {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub positions: BTreeMap<String, Position>,
    #[serde(flatten, default)]
    pub extra: ExtraFields,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Position {
    pub x: f64,
    pub y: f64,
    #[serde(flatten, default)]
    pub extra: ExtraFields,
}
