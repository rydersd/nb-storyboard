//! The stable, file-format layer for storyboard projects.

pub mod compile;
pub mod describe;
pub mod footprint;
pub mod ids;
pub mod model;
pub mod ops;
#[cfg(feature = "render")]
pub mod render;
pub mod validate;

pub use compile::{
    CompileError, CompileWarning, Compiled, CompiledItem, Sample, compile_active, compile_cut,
    compile_story, sample,
};
pub use describe::{
    DescribeError, describe_panel, describe_project, describe_sequence, panel_source_paths,
};
pub use model::*;
pub use ops::{Applied, Op, OpError, OpIssue, Patch, RefTarget, RestoreChange, apply};
#[cfg(feature = "render")]
pub use render::{RenderError, RenderMode, Rendered, Warning, render_cut_sheet, render_panel};
pub use validate::{ValidationIssue, validate};

/// Parses exactly the SVG/CSS color subset accepted by the renderer.
pub fn parse_color(value: &str) -> Option<svgtypes::Color> {
    value.parse().ok()
}
