use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use schemars::JsonSchema;

#[derive(Parser)]
#[command(
    name = "storyboard",
    about = "Create and edit a storyboard project",
    version
)]
pub struct Cli {
    #[arg(long, global = true, help = "Print a readable table for show only")]
    pub pretty: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args, Clone, JsonSchema)]
pub struct WriteArgs {
    #[arg(help = "Project folder")]
    pub dir: PathBuf,
    #[arg(long, help = "Version read before this edit")]
    pub base_version: String,
    #[arg(long, help = "Author: human or agent:NAME; falls back to NB_AUTHOR")]
    pub author: Option<String>,
    #[arg(long, help = "Optional history note for this write")]
    pub note: Option<String>,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(about = "Serve MCP tools over standard input and output")]
    Mcp(McpArgs),
    #[command(about = "Create an empty project")]
    New(NewArgs),
    #[command(about = "Add a scene")]
    AddScene(AddSceneArgs),
    #[command(about = "Add a shot with a take and blank panel")]
    AddShot(AddShotArgs),
    #[command(about = "Add a panel to a take")]
    AddPanel(AddPanelArgs),
    #[command(about = "Add a text layer")]
    AddText(AddTextArgs),
    #[command(about = "Add an image layer")]
    AddImage(AddImageArgs),
    #[command(about = "Copy a file into sources/")]
    Import(ImportArgs),
    #[command(about = "Scan source files and their uses")]
    Sources(SourcesArgs),
    #[command(about = "Add a take to a shot")]
    AddTake(AddTakeArgs),
    #[command(about = "Add a named alternative cut")]
    AddCut(AddCutArgs),
    #[command(about = "Rename a cut")]
    UpdateCut(UpdateCutArgs),
    #[command(about = "Remove a cut")]
    RemoveCut(RemoveCutArgs),
    #[command(about = "Add a shot or nested-cut clip")]
    AddClip(AddClipArgs),
    #[command(about = "Update a clip take or source")]
    UpdateClip(UpdateClipArgs),
    #[command(about = "Remove a clip")]
    RemoveClip(RemoveClipArgs),
    #[command(about = "Set a shot's story fields")]
    SetShot(SetShotArgs),
    #[command(about = "Move a shot to a scene")]
    MoveShot(MoveShotArgs),
    #[command(about = "Choose a shot's take")]
    SelectTake(SelectTakeArgs),
    #[command(about = "Set a clip's trim or fixed slot")]
    Trim(TrimArgs),
    #[command(about = "Undo your most recent commits")]
    Undo(UndoArgs),
    #[command(about = "Show local commit history")]
    History(HistoryArgs),
    #[command(about = "Show the active sequence as a readable table")]
    Show(ShowArgs),
    #[command(about = "Render one panel to PNG")]
    PanelImage(PanelImageArgs),
    #[command(about = "Render a sequence contact sheet to PNG")]
    CutSheet(CutSheetArgs),
    #[command(about = "Add a sticky note")]
    Note(NoteArgs),
    #[command(about = "Reply to a sticky note")]
    Reply(ReplyArgs),
    #[command(about = "Resolve a sticky note")]
    Resolve(ResolveArgs),
    #[command(about = "List sticky notes")]
    Notes(NotesArgs),
    #[command(about = "Print public operation and layer shapes")]
    Schema(SchemaArgs),
}

#[derive(Args, JsonSchema)]
pub struct McpArgs {
    #[arg(help = "Workspace folder containing projects")]
    pub workspace: PathBuf,
}

#[derive(Args, JsonSchema)]
pub struct NewArgs {
    #[arg(help = "New project folder")]
    pub dir: PathBuf,
    #[arg(long, help = "Project title")]
    pub title: String,
    #[arg(
        long,
        default_value_t = 24,
        help = "Frames per second: 24, 25, 30, or 60"
    )]
    pub fps: u32,
    #[arg(long, default_value_t = 1440, help = "Canvas width")]
    pub width: u32,
    #[arg(long, default_value_t = 900, help = "Canvas height")]
    pub height: u32,
}

#[derive(Args, JsonSchema)]
pub struct AddSceneArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Scene title")]
    pub title: String,
    #[arg(long, help = "Optional scene id")]
    pub id: Option<String>,
    #[arg(long, help = "Zero-based insertion index")]
    pub index: Option<usize>,
}
#[derive(Args, JsonSchema)]
pub struct AddShotArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Owning scene id")]
    pub scene: String,
    #[arg(long, help = "Shot title")]
    pub title: String,
    #[arg(long, help = "Optional shot id")]
    pub id: Option<String>,
    #[arg(long, help = "Zero-based insertion index")]
    pub index: Option<usize>,
}
#[derive(Args, JsonSchema)]
pub struct AddPanelArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Shot id")]
    pub shot: Option<String>,
    #[arg(long, help = "Take id")]
    pub take: Option<String>,
    #[arg(long, help = "Sequence position, optionally followed by /PANEL")]
    pub at: Option<String>,
    #[arg(long, help = "Optional panel id")]
    pub id: Option<String>,
    #[arg(long, help = "Seconds from shot start")]
    pub time: Option<f64>,
    #[arg(long, help = "Zero-based insertion index")]
    pub index: Option<usize>,
}
#[derive(Args, JsonSchema)]
pub struct AddTextArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Shot id")]
    pub shot: Option<String>,
    #[arg(long, help = "Take id")]
    pub take: Option<String>,
    #[arg(long, help = "Panel id")]
    pub panel: Option<String>,
    #[arg(long, help = "Sequence position, optionally followed by /PANEL")]
    pub at: Option<String>,
    #[arg(long, help = "Text content")]
    pub text: String,
    #[arg(
        long,
        default_value_t = 0.0,
        allow_negative_numbers = true,
        help = "Layer x"
    )]
    pub x: f64,
    #[arg(
        long,
        default_value_t = 0.0,
        allow_negative_numbers = true,
        help = "Layer y"
    )]
    pub y: f64,
    #[arg(long, default_value_t = 400.0, help = "Layer width")]
    pub width: f64,
    #[arg(long, default_value_t = 100.0, help = "Layer height")]
    pub height: f64,
    #[arg(long, default_value_t = 32.0, help = "Font size")]
    pub font_size: f64,
    #[arg(long, default_value = "#000000", help = "CSS text color")]
    pub color: String,
}
#[derive(Args, JsonSchema)]
pub struct AddImageArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Shot id")]
    pub shot: Option<String>,
    #[arg(long, help = "Take id")]
    pub take: Option<String>,
    #[arg(long, help = "Panel id")]
    pub panel: Option<String>,
    #[arg(long, help = "Sequence position, optionally followed by /PANEL")]
    pub at: Option<String>,
    #[arg(
        long,
        help = "Path below sources/, with or without the sources/ prefix"
    )]
    pub src: String,
    #[arg(
        long,
        default_value_t = 0.0,
        allow_negative_numbers = true,
        help = "Layer x"
    )]
    pub x: f64,
    #[arg(
        long,
        default_value_t = 0.0,
        allow_negative_numbers = true,
        help = "Layer y"
    )]
    pub y: f64,
    #[arg(long, default_value_t = 400.0, help = "Layer width")]
    pub width: f64,
    #[arg(long, default_value_t = 300.0, help = "Layer height")]
    pub height: f64,
    #[arg(
        long,
        default_value = "contain",
        help = "Image fit: contain, cover, or fill"
    )]
    pub fit: String,
}
#[derive(Args, JsonSchema)]
pub struct ImportArgs {
    #[arg(help = "Project folder")]
    pub dir: PathBuf,
    #[arg(help = "File to copy")]
    pub source: PathBuf,
    #[arg(long, help = "Destination below sources/")]
    pub destination: PathBuf,
}
#[derive(Args, JsonSchema)]
pub struct SourcesArgs {
    #[arg(help = "Project folder")]
    pub dir: PathBuf,
}
#[derive(Args, JsonSchema)]
pub struct AddTakeArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Shot id")]
    pub shot: String,
    #[arg(long, help = "Take name")]
    pub name: String,
    #[arg(long, help = "Optional take id")]
    pub id: Option<String>,
    #[arg(long, help = "Zero-based insertion index")]
    pub index: Option<usize>,
}
#[derive(Args, JsonSchema)]
pub struct AddCutArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Cut name")]
    pub name: String,
    #[arg(long, help = "Optional cut id")]
    pub id: Option<String>,
    #[arg(long, help = "Zero-based insertion index")]
    pub index: Option<usize>,
}
#[derive(Args, JsonSchema)]
pub struct UpdateCutArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Cut id")]
    pub cut: String,
    #[arg(long, help = "New cut name")]
    pub name: String,
}
#[derive(Args, JsonSchema)]
pub struct RemoveCutArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Cut id")]
    pub cut: String,
}
#[derive(Args, JsonSchema)]
pub struct AddClipArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Owning cut id")]
    pub cut: String,
    #[arg(long, help = "Source shot id")]
    pub shot: Option<String>,
    #[arg(long, help = "Optional take id for a shot source")]
    pub take: Option<String>,
    #[arg(long = "nested-cut", help = "Source cut id")]
    pub nested_cut: Option<String>,
    #[arg(long, help = "Optional clip id")]
    pub id: Option<String>,
    #[arg(long, help = "Zero-based insertion index")]
    pub index: Option<usize>,
}
#[derive(Args, JsonSchema)]
pub struct UpdateClipArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Owning cut id")]
    pub cut: String,
    #[arg(long, help = "Clip id")]
    pub clip: String,
    #[arg(long, help = "Take id for this shot clip")]
    pub take: Option<String>,
}
#[derive(Args, JsonSchema)]
pub struct RemoveClipArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Owning cut id")]
    pub cut: String,
    #[arg(long, help = "Clip id")]
    pub clip: String,
}
#[derive(Args, JsonSchema)]
pub struct SetShotArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Shot id")]
    pub shot: String,
    #[arg(long, help = "Shot title")]
    pub title: Option<String>,
    #[arg(long, help = "Action text")]
    pub action: Option<String>,
    #[arg(long, help = "Dialogue text")]
    pub dialogue: Option<String>,
    #[arg(long, help = "Speaker name")]
    pub speaker: Option<String>,
    #[arg(long, help = "Camera direction")]
    pub camera: Option<String>,
    #[arg(long, help = "Production notes")]
    pub notes: Option<String>,
    #[arg(long, help = "Duration in seconds")]
    pub duration: Option<f64>,
}
#[derive(Args, JsonSchema)]
pub struct MoveShotArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Shot id")]
    pub shot: String,
    #[arg(long, help = "Destination scene id")]
    pub scene: String,
    #[arg(long, help = "Zero-based destination index")]
    pub index: usize,
}
#[derive(Args, JsonSchema)]
pub struct SelectTakeArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Shot id")]
    pub shot: String,
    #[arg(long, help = "Take id")]
    pub take: String,
}
#[derive(Args, JsonSchema)]
pub struct TrimArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Cut id")]
    pub cut: String,
    #[arg(long, help = "Clip id")]
    pub clip: String,
    #[arg(long, help = "First frame, inclusive, in frames")]
    pub r#in: Option<u64>,
    #[arg(long, help = "End frame, exclusive")]
    pub out: Option<u64>,
    #[arg(long, help = "Fixed clip slot length in frames")]
    pub slot: Option<u64>,
}
#[derive(Args, JsonSchema)]
pub struct UndoArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, default_value_t = 1, help = "Number of your commits to undo")]
    pub steps: usize,
}
#[derive(Args, JsonSchema)]
pub struct HistoryArgs {
    #[arg(help = "Project folder")]
    pub dir: PathBuf,
    #[arg(short = 'n', long, default_value_t = 20, help = "Maximum records")]
    pub limit: usize,
    #[arg(long, help = "Include operations and inverses")]
    pub full: bool,
}
#[derive(Args, JsonSchema)]
pub struct ShowArgs {
    #[arg(help = "Project folder")]
    pub dir: PathBuf,
    #[arg(long, help = "Cut id instead of the active sequence")]
    pub cut: Option<String>,
}
#[derive(Args, JsonSchema)]
pub struct PanelImageArgs {
    #[arg(help = "Project folder")]
    pub dir: PathBuf,
    #[arg(long, help = "Shot id")]
    pub shot: Option<String>,
    #[arg(long, help = "Take id")]
    pub take: Option<String>,
    #[arg(long, help = "Panel id")]
    pub panel: Option<String>,
    #[arg(long, help = "Sequence position, optionally followed by /PANEL")]
    pub at: Option<String>,
    #[arg(long, default_value_t = 960, help = "Output width")]
    pub width: u32,
    #[arg(long, default_value = "stage", help = "Render mode: stage or board")]
    pub mode: String,
    #[arg(long, help = "PNG output path")]
    pub out: Option<PathBuf>,
}
#[derive(Args, JsonSchema)]
pub struct CutSheetArgs {
    #[arg(help = "Project folder")]
    pub dir: PathBuf,
    #[arg(long, help = "Cut id instead of the active sequence")]
    pub cut: Option<String>,
    #[arg(long, default_value_t = 1200, help = "Output width")]
    pub width: u32,
    #[arg(long, help = "PNG output path")]
    pub out: PathBuf,
}
#[derive(Args, JsonSchema)]
pub struct NoteArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Sticky text")]
    pub text: String,
    #[arg(long, default_value = "note", help = "Kind: note, todo, or question")]
    pub kind: String,
    #[arg(long, default_value = "none", help = "Pin text, such as shot:shot-1")]
    pub pin: String,
}
#[derive(Args, JsonSchema)]
pub struct ReplyArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Sticky id to reply to")]
    pub sticky: String,
    #[arg(long, help = "Reply text")]
    pub text: String,
    #[arg(long, default_value = "note", help = "Kind: note, todo, or question")]
    pub kind: String,
}
#[derive(Args, JsonSchema)]
pub struct ResolveArgs {
    #[command(flatten)]
    #[schemars(flatten)]
    pub write: WriteArgs,
    #[arg(long, help = "Sticky id")]
    pub sticky: String,
}
#[derive(Args, JsonSchema)]
pub struct NotesArgs {
    #[arg(help = "Project folder")]
    pub dir: PathBuf,
    #[arg(long, help = "Kind: note, todo, or question")]
    pub kind: Option<String>,
    #[arg(long, help = "Author: human or agent:NAME")]
    pub author: Option<String>,
    #[arg(long, help = "Pin text, such as shot:shot-1")]
    pub pin: Option<String>,
    #[arg(long, help = "Only resolved notes")]
    pub resolved: bool,
    #[arg(long, help = "Only unresolved notes")]
    pub unresolved: bool,
}
#[derive(Args, JsonSchema)]
pub struct SchemaArgs {
    #[arg(help = "Optional snake_case operation name")]
    pub op: Option<String>,
}
