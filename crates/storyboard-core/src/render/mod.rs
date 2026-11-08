//! Deterministic panel and sequence rendering using the bundled Noto Sans font.

use std::path::{Path, PathBuf};
use std::{cell::RefCell, fs};

use cosmic_text::{
    Align, Attrs, Buffer, Color as TextColor, Family, FontSystem, Metrics, Shaping, SwashCache,
    Weight, fontdb,
};
use image::{
    ColorType, ImageEncoder, ImageReader,
    codecs::png::{CompressionType, FilterType, PngEncoder},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tiny_skia::{
    Color, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke as SkiaStroke, Transform,
};

use crate::{Compiled, Layer, Panel, Project, Shot, Take, parse_color};

const MAX_CANVAS_PIXELS: u64 = 64_000_000;
// Dense paths keep every prepared vertex and segment, but rasterize through a bounded
// intermediate so pathological self-crossing paths cannot monopolize the renderer.
const MAX_STROKE_PATH_CHUNK_POINTS: usize = 512;
const MIN_STROKE_PATH_POINTS: usize = 50_000;
/// Total stroke ink (length x width, in output pixels) a panel may paint, as a multiple of its
/// pixel area. Enough to cover the canvas several times; bounds pathological zigzag strokes.
const STROKE_INK_AREA_MULTIPLE: f64 = 4.0;
const STROKE_POINTS_PER_OUTPUT_EDGE_PIXEL: usize = 64;
const FONT: &[u8] = include_bytes!("NotoSans-Variable.ttf");
const _: &[u8] = include_bytes!("OFL.txt");

thread_local! { static LAST_IMAGE_RASTER_TARGET: RefCell<Option<(u32, u32)>> = const { RefCell::new(None) }; }

/// Test-only observation point for the image intermediate selected by the most recent render on
/// this thread. It is deliberately hidden from generated documentation.
#[doc(hidden)]
pub fn last_image_raster_target_size() -> Option<(u32, u32)> {
    LAST_IMAGE_RASTER_TARGET.with(|target| *target.borrow())
}

fn record_image_raster_target_size(width: u32, height: u32) {
    LAST_IMAGE_RASTER_TARGET.with(|target| *target.borrow_mut() = Some((width, height)));
}

/// The stage is the panel image; board adds its contextual caption beneath it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderMode {
    Stage,
    Board,
}

/// A non-fatal rendering condition. The PNG is still usable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    pub field: String,
    pub message: String,
}

/// A deterministic PNG result, including its actual output dimensions and non-fatal warnings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rendered {
    #[serde(skip_serializing, default)]
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub warnings: Vec<Warning>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Error)]
#[error("{message}")]
pub struct RenderError {
    pub kind: String,
    pub message: String,
    pub hint: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub valid: Vec<String>,
}

impl RenderError {
    fn width() -> Self {
        Self {
            kind: "width".into(),
            message: "output width must be between 1 and 8192".into(),
            hint: "choose a width in the supported range".into(),
            valid: vec!["1..=8192".into()],
        }
    }

    fn canvas() -> Self {
        Self {
            kind: "canvas".into(),
            message: "render canvas exceeds 64 megapixels".into(),
            hint: "choose a smaller output size".into(),
            valid: vec!["at most 64000000 pixels".into()],
        }
    }

    fn missing(kind: &str, message: String, hint: String) -> Self {
        Self {
            kind: kind.into(),
            message,
            hint,
            valid: Vec::new(),
        }
    }

    fn encode(message: String) -> Self {
        Self::missing(
            "encode",
            format!("could not encode PNG: {message}"),
            "try rendering again".into(),
        )
    }
}

/// Renders a selected panel. Source failures and unsupported layer values become warnings.
pub fn render_panel(
    project: &Project,
    project_root: &Path,
    shot_id: &str,
    take_id: &str,
    panel_id: &str,
    width: u32,
    mode: RenderMode,
) -> Result<Rendered, RenderError> {
    let (_, shot, take, panel) = find_panel(project, shot_id, take_id, panel_id)?;
    let (actual_width, canvas_warning) = fit_panel_width(project, width, mode)?;
    let (pixmap, mut warnings) = render_one(
        project,
        project_root,
        PanelContext {
            shot,
            take,
            panel,
            number: shot_number(project, shot_id),
            document_path: panel_document_path(project, shot_id, take_id, panel_id),
        },
        actual_width,
        mode,
    )?;
    if let Some(message) = canvas_warning {
        warnings.push(Warning {
            field: "project.format".into(),
            message,
        });
    }
    encoded(pixmap, warnings)
}

/// Renders one tile for every compiled sequence item in playback order.
pub fn render_cut_sheet(
    project: &Project,
    project_root: &Path,
    sequence: &Compiled,
    width: u32,
) -> Result<Rendered, RenderError> {
    check_width(width)?;
    let count = sequence.items.len().max(1);
    let (actual_width, canvas_warning) = fit_cut_sheet_width(project, count, width)?;
    let layout = cut_sheet_layout(project, count, actual_width)?;
    let CutSheetLayout {
        columns,
        gutter,
        tile_width,
        tile_height,
        tile_caption_height,
        row_height,
        height,
    } = layout;
    let mut sheet = Pixmap::new(actual_width, height).ok_or_else(RenderError::canvas)?;
    sheet.fill(Color::from_rgba8(232, 229, 222, 255));
    let mut warnings = Vec::new();
    if let Some(message) = canvas_warning {
        warnings.push(Warning {
            field: "project.format".into(),
            message,
        });
    }
    for (index, item) in sequence.items.iter().enumerate() {
        let Some((_, shot, take)) = find_take(project, &item.shot, &item.take) else {
            warnings.push(Warning {
                field: "project.scenes".into(),
                message: "compiled shot or take is no longer present".into(),
            });
            continue;
        };
        let offset_seconds = item.offset as f64 / f64::from(project.fps.max(1));
        let Some(panel) = take
            .panels
            .iter()
            .rev()
            .find(|panel| panel.time <= offset_seconds)
        else {
            warnings.push(Warning {
                field: "project.scenes".into(),
                message: "take has no panel at this time".into(),
            });
            continue;
        };
        let (tile, tile_warnings) = render_one(
            project,
            project_root,
            PanelContext {
                shot,
                take,
                panel,
                number: shot_number(project, &shot.id),
                document_path: panel_document_path(project, &shot.id, &take.id, &panel.id),
            },
            tile_width,
            RenderMode::Stage,
        )?;
        warnings.extend(tile_warnings);
        let column = u32::try_from(index).map_err(|_| RenderError::canvas())? % columns;
        let row = u32::try_from(index).map_err(|_| RenderError::canvas())? / columns;
        sheet.draw_pixmap(
            i32::try_from(gutter + column * (tile_width + gutter))
                .map_err(|_| RenderError::canvas())?,
            i32::try_from(gutter + row * (row_height + gutter))
                .map_err(|_| RenderError::canvas())?,
            tile.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
        let caption_y = gutter + row * (row_height + gutter) + tile_height;
        fill_rect(
            &mut sheet,
            Rect::from_xywh(
                (gutter + column * (tile_width + gutter)) as f32,
                caption_y as f32,
                tile_width as f32,
                tile_caption_height as f32,
            )
            .expect("cut-sheet caption has size"),
            Color::from_rgba8(250, 248, 242, 255),
        );
        let caption = format!("{}  {}", shot_number(project, &shot.id), shot.title);
        draw_text(
            &mut sheet,
            &caption,
            (gutter + column * (tile_width + gutter) + 4) as f32,
            (caption_y + 3) as f32,
            tile_width.saturating_sub(8) as f32,
            tile_caption_height.saturating_sub(6) as f32,
            (tile_width as f32 / 10.0).clamp(10.0, 20.0),
            "left",
            false,
            [30, 30, 30, 255],
        );
    }
    encoded(sheet, warnings)
}

#[derive(Clone, Copy)]
struct CutSheetLayout {
    columns: u32,
    gutter: u32,
    tile_width: u32,
    tile_height: u32,
    tile_caption_height: u32,
    row_height: u32,
    height: u32,
}

fn cut_sheet_layout(
    project: &Project,
    count: usize,
    width: u32,
) -> Result<CutSheetLayout, RenderError> {
    let columns = u32::try_from(count.min(3).min(width as usize).max(1))
        .map_err(|_| RenderError::canvas())?;
    let gutter = if width > columns.saturating_mul(3) {
        (width / 80).clamp(2, 24)
    } else {
        0
    };
    let tile_width = width
        .saturating_sub(gutter.saturating_mul(columns.saturating_add(1)))
        .checked_div(columns)
        .unwrap_or(1)
        .max(1);
    let tile_height = stage_height(project, tile_width)?;
    let tile_caption_height = (tile_width / 5).clamp(20, 60);
    let row_height = tile_height
        .checked_add(tile_caption_height)
        .ok_or_else(RenderError::canvas)?;
    let rows =
        u32::try_from(count.div_ceil(columns as usize)).map_err(|_| RenderError::canvas())?;
    let height = gutter
        .checked_add(
            rows.checked_mul(
                row_height
                    .checked_add(gutter)
                    .ok_or_else(RenderError::canvas)?,
            )
            .ok_or_else(RenderError::canvas)?,
        )
        .ok_or_else(RenderError::canvas)?;
    Ok(CutSheetLayout {
        columns,
        gutter,
        tile_width,
        tile_height,
        tile_caption_height,
        row_height,
        height,
    })
}

fn fit_cut_sheet_width(
    project: &Project,
    count: usize,
    requested_width: u32,
) -> Result<(u32, Option<String>), RenderError> {
    let requested = cut_sheet_layout(project, count, requested_width)?;
    if u64::from(requested_width) * u64::from(requested.height) <= MAX_CANVAS_PIXELS {
        return Ok((requested_width, None));
    }
    let factor = (MAX_CANVAS_PIXELS as f64
        / (u64::from(requested_width) * u64::from(requested.height)) as f64)
        .sqrt();
    let mut actual_width = (requested_width as f64 * factor).floor().max(1.0) as u32;
    while actual_width > 1 {
        let actual = cut_sheet_layout(project, count, actual_width)?;
        if u64::from(actual_width) * u64::from(actual.height) <= MAX_CANVAS_PIXELS {
            return Ok((
                actual_width,
                Some(format!(
                    "requested {}×{}; rendered {}×{} to stay within the 64 megapixel limit",
                    requested_width, requested.height, actual_width, actual.height
                )),
            ));
        }
        actual_width -= 1;
    }
    Err(RenderError::canvas())
}

fn encoded(pixmap: Pixmap, warnings: Vec<Warning>) -> Result<Rendered, RenderError> {
    let (width, height) = (pixmap.width(), pixmap.height());
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::NoFilter)
        .write_image(pixmap.data(), width, height, ColorType::Rgba8.into())
        .map_err(|error| RenderError::encode(error.to_string()))?;
    Ok(Rendered {
        png,
        width,
        height,
        warnings,
    })
}

fn find_panel<'a>(
    project: &'a Project,
    shot_id: &str,
    take_id: &str,
    panel_id: &str,
) -> Result<(&'a crate::Scene, &'a Shot, &'a Take, &'a Panel), RenderError> {
    let Some((scene, shot)) = project.scenes.iter().find_map(|scene| {
        scene
            .shots
            .iter()
            .find(|shot| shot.id == shot_id)
            .map(|shot| (scene, shot))
    }) else {
        return Err(RenderError::missing(
            "shot",
            format!("unknown shot {shot_id}"),
            "choose an existing shot".into(),
        ));
    };
    let take = shot
        .takes
        .iter()
        .find(|take| take.id == take_id)
        .ok_or_else(|| {
            RenderError::missing(
                "take",
                format!("unknown take {take_id} for shot {shot_id}"),
                "choose a take belonging to the shot".into(),
            )
        })?;
    let panel = take
        .panels
        .iter()
        .find(|panel| panel.id == panel_id)
        .ok_or_else(|| {
            RenderError::missing(
                "panel",
                format!("unknown panel {panel_id} for {shot_id}/{take_id}"),
                "choose a panel belonging to the take".into(),
            )
        })?;
    Ok((scene, shot, take, panel))
}

fn find_take<'a>(
    project: &'a Project,
    shot_id: &str,
    take_id: &str,
) -> Option<(&'a crate::Scene, &'a Shot, &'a Take)> {
    project.scenes.iter().find_map(|scene| {
        scene
            .shots
            .iter()
            .find(|shot| shot.id == shot_id)
            .and_then(|shot| {
                shot.takes
                    .iter()
                    .find(|take| take.id == take_id)
                    .map(|take| (scene, shot, take))
            })
    })
}

fn shot_number(project: &Project, shot_id: &str) -> usize {
    project
        .scenes
        .iter()
        .flat_map(|scene| &scene.shots)
        .position(|shot| shot.id == shot_id)
        .map_or(0, |index| index + 1)
}

fn story_frame_range(project: &Project, shot_id: &str) -> (u64, u64) {
    let mut start = 0_u64;
    for shot in project.scenes.iter().flat_map(|scene| &scene.shots) {
        let frames = (shot.duration.max(0.0) * f64::from(project.fps.max(1))).round() as u64;
        let end = start.saturating_add(frames);
        if shot.id == shot_id {
            return (start, end);
        }
        start = end;
    }
    (0, 0)
}

struct PanelContext<'a> {
    shot: &'a Shot,
    take: &'a Take,
    panel: &'a Panel,
    number: usize,
    document_path: String,
}

fn panel_document_path(project: &Project, shot_id: &str, take_id: &str, panel_id: &str) -> String {
    for (scene_index, scene) in project.scenes.iter().enumerate() {
        for (shot_index, shot) in scene.shots.iter().enumerate() {
            if shot.id != shot_id {
                continue;
            }
            for (take_index, take) in shot.takes.iter().enumerate() {
                if take.id != take_id {
                    continue;
                }
                if let Some(panel_index) = take.panels.iter().position(|panel| panel.id == panel_id)
                {
                    return format!(
                        "scenes[{scene_index}].shots[{shot_index}].takes[{take_index}].panels[{panel_index}]"
                    );
                }
            }
        }
    }
    "project".into()
}

fn render_one(
    project: &Project,
    root: &Path,
    context: PanelContext<'_>,
    width: u32,
    mode: RenderMode,
) -> Result<(Pixmap, Vec<Warning>), RenderError> {
    check_width(width)?;
    let image_height = stage_height(project, width)?;
    let caption_height = if mode == RenderMode::Board {
        (width / 5).clamp(48, 240)
    } else {
        0
    };
    let height = image_height
        .checked_add(caption_height)
        .ok_or_else(RenderError::canvas)?;
    check_canvas(width, height)?;
    let mut pixmap = Pixmap::new(width, height).ok_or_else(RenderError::canvas)?;
    let mut warnings = Vec::new();
    pixmap.fill(color_or_default(
        &context.panel.background,
        &format!("{}.background", context.document_path),
        &mut warnings,
        Color::from_rgba8(245, 241, 232, 255),
    ));
    let scale = width as f32 / project.format.width.max(1) as f32;
    if context.panel.layers.is_empty() && context.panel.strokes.is_empty() {
        empty_placeholder(&mut pixmap, width, image_height);
    }
    for (index, layer) in context.panel.layers.iter().enumerate() {
        if !layer.hidden {
            draw_layer(
                &mut pixmap,
                root,
                layer,
                scale,
                image_height,
                index,
                &format!("{}.layers", context.document_path),
                &mut warnings,
            );
        }
    }
    draw_panel_strokes(
        &mut pixmap,
        context.panel,
        scale,
        image_height,
        &format!("{}.strokes", context.document_path),
        &mut warnings,
    )?;
    if caption_height > 0 {
        draw_caption(
            &mut pixmap,
            project,
            context,
            image_height,
            caption_height,
            width,
            scale,
            &mut warnings,
        );
    }
    Ok((pixmap, warnings))
}

fn check_width(width: u32) -> Result<(), RenderError> {
    if !(1..=8192).contains(&width) {
        Err(RenderError::width())
    } else {
        Ok(())
    }
}
fn stage_height(project: &Project, width: u32) -> Result<u32, RenderError> {
    let height = u64::from(width)
        .checked_mul(u64::from(project.format.height.max(1)))
        .ok_or_else(RenderError::canvas)?
        / u64::from(project.format.width.max(1));
    u32::try_from(height.max(1)).map_err(|_| RenderError::canvas())
}
fn check_canvas(width: u32, height: u32) -> Result<(), RenderError> {
    if u64::from(width) * u64::from(height) > MAX_CANVAS_PIXELS {
        Err(RenderError::canvas())
    } else {
        Ok(())
    }
}

fn panel_dimensions(
    project: &Project,
    width: u32,
    mode: RenderMode,
) -> Result<(u32, u32), RenderError> {
    let image_height = stage_height(project, width)?;
    let caption_height = if mode == RenderMode::Board {
        (width / 5).clamp(48, 240)
    } else {
        0
    };
    Ok((
        width,
        image_height
            .checked_add(caption_height)
            .ok_or_else(RenderError::canvas)?,
    ))
}

fn fit_panel_width(
    project: &Project,
    requested_width: u32,
    mode: RenderMode,
) -> Result<(u32, Option<String>), RenderError> {
    check_width(requested_width)?;
    let requested = panel_dimensions(project, requested_width, mode)?;
    if u64::from(requested.0) * u64::from(requested.1) <= MAX_CANVAS_PIXELS {
        return Ok((requested_width, None));
    }
    let factor = (MAX_CANVAS_PIXELS as f64
        / (u64::from(requested.0) * u64::from(requested.1)) as f64)
        .sqrt();
    let mut actual_width = (requested_width as f64 * factor).floor().max(1.0) as u32;
    while actual_width > 1 {
        let (_, actual_height) = panel_dimensions(project, actual_width, mode)?;
        if u64::from(actual_width) * u64::from(actual_height) <= MAX_CANVAS_PIXELS {
            return Ok((
                actual_width,
                Some(format!(
                    "requested {}×{}; rendered {}×{} to stay within the 64 megapixel limit",
                    requested.0, requested.1, actual_width, actual_height
                )),
            ));
        }
        actual_width -= 1;
    }
    Err(RenderError::canvas())
}

fn color_or_default(
    value: &str,
    field: &str,
    warnings: &mut Vec<Warning>,
    default: Color,
) -> Color {
    if value.is_empty() {
        return default;
    }
    match parse_color(value) {
        Some(color) => Color::from_rgba8(color.red, color.green, color.blue, color.alpha),
        None => {
            warnings.push(Warning {
                field: field.into(),
                message: format!("unsupported CSS color {value:?}"),
            });
            default
        }
    }
}
fn opacity(color: Color, value: f64) -> Color {
    Color::from_rgba(
        color.red(),
        color.green(),
        color.blue(),
        (color.alpha() * value.clamp(0.0, 1.0) as f32).clamp(0.0, 1.0),
    )
    .unwrap_or(color)
}
fn rect(layer: &Layer, scale: f32, image_height: u32) -> Option<Rect> {
    let (x, y, width, height) = (
        layer.x as f32 * scale,
        layer.y as f32 * scale,
        layer.width as f32 * scale,
        layer.height as f32 * scale,
    );
    if ![x, y, width, height].into_iter().all(f32::is_finite)
        || width <= 0.0
        || height <= 0.0
        || y >= image_height as f32
    {
        None
    } else {
        Rect::from_xywh(x, y, width, height)
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_layer(
    pixmap: &mut Pixmap,
    root: &Path,
    layer: &Layer,
    scale: f32,
    image_height: u32,
    index: usize,
    field_prefix: &str,
    warnings: &mut Vec<Warning>,
) {
    let field = format!("{field_prefix}[{index}]");
    let Some(rect) = rect(layer, scale, image_height) else {
        warnings.push(Warning {
            field,
            message: "unsupported or off-canvas layer geometry".into(),
        });
        return;
    };
    match layer.layer_type.as_str() {
        "solid" => fill_rect(
            pixmap,
            rect,
            opacity(
                color_or_default(
                    layer.fill.as_deref().unwrap_or(""),
                    &format!("{field}.fill"),
                    warnings,
                    Color::from_rgba8(180, 180, 180, 255),
                ),
                layer.opacity,
            ),
        ),
        "box" => draw_box(pixmap, rect, layer, scale),
        "text" => {
            let requested_size = layer.font_size.unwrap_or(32.0) as f32 * scale;
            if !requested_size.is_finite() || requested_size <= 0.0 {
                warnings.push(Warning {
                    field,
                    message: "unsupported font size".into(),
                });
                return;
            }
            let maximum_size = image_height as f32;
            let size = requested_size.min(maximum_size.max(1.0));
            if size < requested_size {
                warnings.push(Warning {
                    field: format!("{field}.font_size"),
                    message: format!(
                        "font size {:.3}px was clamped to {:.3}px for the canvas",
                        requested_size, size
                    ),
                });
            }
            let text = bound_text(layer.text.as_deref().unwrap_or(""), &field, warnings);
            let color = color_or_default(
                layer.color.as_deref().unwrap_or("black"),
                &format!("{field}.color"),
                warnings,
                Color::from_rgba8(0, 0, 0, 255),
            );
            if text_overflows(
                &text,
                rect.width(),
                rect.height(),
                size,
                layer.align.as_deref(),
                layer.weight.as_deref(),
            ) {
                warnings.push(Warning {
                    field: format!("{field}.overflow"),
                    message: "text overflows its layer bounds".into(),
                });
            }
            draw_text(
                pixmap,
                &text,
                rect.x(),
                rect.y(),
                rect.width(),
                rect.height(),
                size,
                layer.align.as_deref().unwrap_or("left"),
                layer.weight.as_deref().unwrap_or("regular") == "bold",
                rgba(color, layer.opacity),
            );
        }
        "image" => draw_image(pixmap, root, layer, rect, scale, &field, warnings),
        other => warnings.push(Warning {
            field,
            message: format!("unsupported layer type {other:?}"),
        }),
    }
}
fn fill_rect(pixmap: &mut Pixmap, rect: Rect, color: Color) {
    let mut paint = Paint::default();
    paint.set_color(color);
    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
}
fn draw_box(pixmap: &mut Pixmap, rect: Rect, layer: &Layer, scale: f32) {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(80, 80, 80, 255));
    let stroke = SkiaStroke {
        width: (2.0 * scale).max(0.5),
        ..SkiaStroke::default()
    };
    stroke_rectangle(pixmap, rect, &paint, &stroke);
    if !layer.label.is_empty() {
        draw_text(
            pixmap,
            &layer.label,
            rect.x() + 4.0,
            rect.y() + 4.0,
            rect.width() - 8.0,
            rect.height() - 8.0,
            (14.0 * scale).max(8.0),
            "left",
            false,
            [80, 80, 80, 255],
        );
    }
}

fn draw_image(
    pixmap: &mut Pixmap,
    root: &Path,
    layer: &Layer,
    rect: Rect,
    scale: f32,
    field: &str,
    warnings: &mut Vec<Warning>,
) {
    let Some(src) = layer.src.as_deref() else {
        warning_box(pixmap, rect, layer, "missing image source", scale);
        warnings.push(Warning {
            field: field.into(),
            message: "image layer has no source".into(),
        });
        return;
    };
    let Ok(path) = contained_file(root, src) else {
        warning_box(pixmap, rect, layer, &format!("missing: {src}"), scale);
        warnings.push(Warning {
            field: format!("{field}.src"),
            message: format!("could not decode source {src:?}"),
        });
        return;
    };
    let svg = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"));
    let source = if svg {
        None
    } else {
        match load_image(&path, layer.crop.as_ref()) {
            Ok(image) => Some(image),
            Err(()) => {
                warning_box(pixmap, rect, layer, &format!("missing: {src}"), scale);
                warnings.push(Warning {
                    field: format!("{field}.src"),
                    message: format!("could not decode source {src:?}"),
                });
                return;
            }
        }
    };
    let (source_width, source_height) = match &source {
        Some(image) => (image.width() as f32, image.height() as f32),
        None => match svg_dimensions(&path) {
            Ok(size) => size,
            Err(()) => {
                warning_box(pixmap, rect, layer, &format!("missing: {src}"), scale);
                warnings.push(Warning {
                    field: format!("{field}.src"),
                    message: format!("could not decode source {src:?}"),
                });
                return;
            }
        },
    };
    let fit = layer.fit.as_deref().unwrap_or("contain");
    let scale = match fit {
        "contain" => (rect.width() / source_width).min(rect.height() / source_height),
        "cover" => (rect.width() / source_width).max(rect.height() / source_height),
        "fill" => 0.0,
        other => {
            warnings.push(Warning {
                field: format!("{field}.fit"),
                message: format!("unsupported image fit {other:?}; using contain"),
            });
            (rect.width() / source_width).min(rect.height() / source_height)
        }
    };
    let (draw_width, draw_height) = if fit == "fill" {
        (rect.width(), rect.height())
    } else {
        (source_width * scale, source_height * scale)
    };
    let (x, y) = (
        rect.x() + (rect.width() - draw_width) / 2.0,
        rect.y() + (rect.height() - draw_height) / 2.0,
    );
    let Some((visible, clipped_to_canvas)) =
        visible_image_rect(pixmap, rect, x, y, draw_width, draw_height)
    else {
        warnings.push(Warning {
            field: field.into(),
            message: "image layer is outside the canvas".into(),
        });
        return;
    };
    if clipped_to_canvas {
        warnings.push(Warning {
            field: field.into(),
            message: "image raster work was clipped to the canvas".into(),
        });
    }
    let target_width = visible.width().ceil().max(1.0) as u32;
    let target_height = visible.height().ceil().max(1.0) as u32;
    record_image_raster_target_size(target_width, target_height);
    let source_left = (visible.x() - x) * source_width / draw_width;
    let source_top = (visible.y() - y) * source_height / draw_height;
    let source_visible_width = visible.width() * source_width / draw_width;
    let source_visible_height = visible.height() * source_height / draw_height;
    let source = match source {
        Some(source) => resample_area(
            &source,
            source_left,
            source_top,
            source_visible_width,
            source_visible_height,
            target_width,
            target_height,
        ),
        None => rasterize_svg(
            &path,
            ImageRasterPlacement {
                x,
                y,
                draw_width,
                draw_height,
                visible,
                target_width,
                target_height,
            },
        ),
    };
    let Some(source) = source else {
        warning_box(pixmap, rect, layer, &format!("missing: {src}"), scale);
        warnings.push(Warning {
            field: format!("{field}.src"),
            message: format!("could not decode source {src:?}"),
        });
        return;
    };
    let (source_width, source_height) = (source.width() as f32, source.height() as f32);
    pixmap.draw_pixmap(
        0,
        0,
        source.as_ref(),
        &PixmapPaint {
            opacity: layer.opacity.clamp(0.0, 1.0) as f32,
            quality: tiny_skia::FilterQuality::Bicubic,
            ..Default::default()
        },
        Transform::from_row(
            visible.width() / source_width,
            0.0,
            0.0,
            visible.height() / source_height,
            visible.x(),
            visible.y(),
        ),
        None,
    );
}

fn visible_image_rect(
    pixmap: &Pixmap,
    layer: Rect,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> Option<(Rect, bool)> {
    let layer_left = x.max(layer.x());
    let layer_top = y.max(layer.y());
    let layer_right = (x + width).min(layer.x() + layer.width());
    let layer_bottom = (y + height).min(layer.y() + layer.height());
    let canvas_left = layer_left.max(0.0);
    let canvas_top = layer_top.max(0.0);
    let canvas_right = layer_right.min(pixmap.width() as f32);
    let canvas_bottom = layer_bottom.min(pixmap.height() as f32);
    (canvas_left < canvas_right && canvas_top < canvas_bottom)
        .then(|| {
            (
                Rect::from_ltrb(canvas_left, canvas_top, canvas_right, canvas_bottom),
                canvas_left > layer_left
                    || canvas_top > layer_top
                    || canvas_right < layer_right
                    || canvas_bottom < layer_bottom,
            )
        })
        .and_then(|(rect, clipped)| rect.map(|rect| (rect, clipped)))
}
fn contained_file(root: &Path, src: &str) -> Result<PathBuf, ()> {
    if Path::new(src).is_absolute() {
        return Err(());
    }
    let root = root.canonicalize().map_err(|_| ())?;
    let path = root.join(src).canonicalize().map_err(|_| ())?;
    path.starts_with(root).then_some(path).ok_or(())
}
fn load_image(path: &Path, crop: Option<&crate::Crop>) -> Result<Pixmap, ()> {
    let image = ImageReader::open(path)
        .map_err(|_| ())?
        .with_guessed_format()
        .map_err(|_| ())?
        .decode()
        .map_err(|_| ())?;
    let image = match crop {
        None => image,
        Some(crop) => {
            if ![crop.x, crop.y, crop.width, crop.height]
                .into_iter()
                .all(f64::is_finite)
                || crop.width <= 0.0
                || crop.height <= 0.0
            {
                return Err(());
            }
            let left = crop.x.max(0.0).floor() as u32;
            let top = crop.y.max(0.0).floor() as u32;
            if left >= image.width() || top >= image.height() {
                return Err(());
            }
            let width = crop.width.ceil() as u32;
            let height = crop.height.ceil() as u32;
            image.crop_imm(
                left,
                top,
                width.min(image.width() - left),
                height.min(image.height() - top),
            )
        }
    }
    .into_rgba8();
    let (width, height) = image.dimensions();
    Pixmap::from_vec(
        image.into_raw(),
        tiny_skia::IntSize::from_wh(width, height).ok_or(())?,
    )
    .ok_or(())
}

fn svg_dimensions(path: &Path) -> Result<(f32, f32), ()> {
    let data = fs::read(path).map_err(|_| ())?;
    let tree =
        resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default()).map_err(|_| ())?;
    let size = tree.size();
    (size.width().is_finite()
        && size.height().is_finite()
        && size.width() > 0.0
        && size.height() > 0.0)
        .then_some((size.width(), size.height()))
        .ok_or(())
}

struct ImageRasterPlacement {
    x: f32,
    y: f32,
    draw_width: f32,
    draw_height: f32,
    visible: Rect,
    target_width: u32,
    target_height: u32,
}

fn rasterize_svg(path: &Path, placement: ImageRasterPlacement) -> Option<Pixmap> {
    let data = fs::read(path).ok()?;
    let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    let mut pixmap = Pixmap::new(placement.target_width, placement.target_height)?;
    resvg::render(
        &tree,
        Transform::from_row(
            placement.draw_width / size.width(),
            0.0,
            0.0,
            placement.draw_height / size.height(),
            placement.x - placement.visible.x(),
            placement.y - placement.visible.y(),
        ),
        &mut pixmap.as_mut(),
    );
    Some(pixmap)
}

/// Uses area averaging before the final draw. A one-pixel source line therefore still affects
/// its destination pixel instead of disappearing between bicubic sample points.
fn resample_area(
    source: &Pixmap,
    source_left: f32,
    source_top: f32,
    source_width: f32,
    source_height: f32,
    target_width: u32,
    target_height: u32,
) -> Option<Pixmap> {
    if source_left == 0.0
        && source_top == 0.0
        && source_width == source.width() as f32
        && source_height == source.height() as f32
        && target_width >= source.width()
        && target_height >= source.height()
    {
        return Some(source.clone());
    }
    let mut target = Pixmap::new(target_width, target_height)?;
    for target_y in 0..target_height {
        let top = source_top + target_y as f32 * source_height / target_height as f32;
        let bottom = source_top + (target_y + 1) as f32 * source_height / target_height as f32;
        for target_x in 0..target_width {
            let left = source_left + target_x as f32 * source_width / target_width as f32;
            let right = source_left + (target_x + 1) as f32 * source_width / target_width as f32;
            let mut channels = [0.0; 4];
            let mut weight = 0.0;
            for source_y in
                top.floor().max(0.0) as u32..bottom.ceil().min(source.height() as f32) as u32
            {
                let y_weight =
                    (bottom.min((source_y + 1) as f32) - top.max(source_y as f32)).max(0.0);
                for source_x in
                    left.floor().max(0.0) as u32..right.ceil().min(source.width() as f32) as u32
                {
                    let x_weight =
                        (right.min((source_x + 1) as f32) - left.max(source_x as f32)).max(0.0);
                    let sample_weight = x_weight * y_weight;
                    let offset =
                        usize::try_from((source_y * source.width() + source_x) * 4).ok()?;
                    for (channel, value) in
                        channels.iter_mut().zip(&source.data()[offset..offset + 4])
                    {
                        *channel += f32::from(*value) * sample_weight;
                    }
                    weight += sample_weight;
                }
            }
            let offset = usize::try_from((target_y * target_width + target_x) * 4).ok()?;
            for (channel, value) in target.data_mut()[offset..offset + 4]
                .iter_mut()
                .zip(channels)
            {
                *channel = if weight > 0.0 {
                    (value / weight).round() as u8
                } else {
                    0
                };
            }
        }
    }
    Some(target)
}
fn warning_box(pixmap: &mut Pixmap, rect: Rect, layer: &Layer, label: &str, scale: f32) {
    fill_rect(pixmap, rect, Color::from_rgba8(255, 238, 210, 255));
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(170, 60, 25, 255));
    let stroke = SkiaStroke {
        width: (2.0 * scale).max(1.0),
        ..SkiaStroke::default()
    };
    stroke_rectangle(pixmap, rect, &paint, &stroke);
    let text = if layer.label.is_empty() {
        label
    } else {
        &layer.label
    };
    draw_text(
        pixmap,
        text,
        rect.x() + 4.0,
        rect.y() + 4.0,
        rect.width() - 8.0,
        rect.height() - 8.0,
        (14.0 * scale).max(10.0),
        "left",
        false,
        [120, 35, 15, 255],
    );
}

fn draw_panel_strokes(
    pixmap: &mut Pixmap,
    panel: &Panel,
    output_scale: f32,
    image_height: u32,
    field_prefix: &str,
    warnings: &mut Vec<Warning>,
) -> Result<(), RenderError> {
    draw_strokes(
        pixmap,
        panel,
        StrokeRenderPlan {
            draw_scale: output_scale,
            simplify_scale: output_scale,
            draw_offset: 0.0,
            budget_size: (pixmap.width(), image_height),
            minimum_width: 0.5,
        },
        field_prefix,
        warnings,
    );
    Ok(())
}

#[derive(Clone, Copy)]
struct StrokeRenderPlan {
    draw_scale: f32,
    simplify_scale: f32,
    draw_offset: f32,
    budget_size: (u32, u32),
    minimum_width: f32,
}

fn draw_strokes(
    pixmap: &mut Pixmap,
    panel: &Panel,
    plan: StrokeRenderPlan,
    field_prefix: &str,
    warnings: &mut Vec<Warning>,
) {
    let mut remaining_points = stroke_point_budget(plan.budget_size.0, plan.budget_size.1);
    let mut remaining_ink =
        f64::from(plan.budget_size.0) * f64::from(plan.budget_size.1) * STROKE_INK_AREA_MULTIPLE;
    // This iterator consumes exactly one input stroke per iteration, so preparation terminates
    // after at most `panel.strokes.len()` iterations.
    for (index, stroke) in panel.strokes.iter().enumerate() {
        if stroke.points.len() < 2 {
            warnings.push(Warning {
                field: format!("{field_prefix}[{index}]"),
                message: "stroke needs at least two points".into(),
            });
            continue;
        }
        let field = format!("{field_prefix}[{index}]");
        let points = simplify_stroke_points(
            &stroke.points,
            plan.simplify_scale,
            plan.draw_scale,
            plan.draw_offset,
        );
        if points.len() > remaining_points {
            let dropped = panel.strokes.len() - index;
            let noun = if dropped == 1 { "stroke" } else { "strokes" };
            warnings.push(Warning {
                field: field_prefix.into(),
                message: format!(
                    "{dropped} trailing {noun} dropped to stay within the panel-wide point limit"
                ),
            });
            break;
        }
        remaining_points -= points.len();
        if points.len() < 2 {
            warnings.push(Warning {
                field,
                message: "stroke has fewer than two points after half-pixel simplification".into(),
            });
            continue;
        }
        let mut paint = Paint::default();
        paint.set_color(color_or_default(
            &stroke.color,
            &format!("{field_prefix}[{index}].color"),
            warnings,
            Color::from_rgba8(0, 0, 0, 255),
        ));
        let style = SkiaStroke {
            width: (stroke.width as f32 * plan.draw_scale).max(plan.minimum_width),
            ..SkiaStroke::default()
        };
        if !style.width.is_finite() {
            warnings.push(Warning {
                field: format!("{field_prefix}[{index}].width"),
                message: "unsupported stroke width".into(),
            });
            continue;
        }
        let kept = ink_prefix_len(&points, f64::from(style.width), &mut remaining_ink);
        if kept < points.len() {
            warnings.push(Warning {
                field: field.clone(),
                message: format!(
                    "stroke cut after {kept} of {} points to stay within the panel-wide ink limit",
                    points.len()
                ),
            });
        }
        if kept >= 2 && !raster_stroke_points(pixmap, &points[..kept], &paint, &style) {
            warnings.push(Warning {
                field,
                message: "unsupported stroke points".into(),
            });
        }
        if kept < points.len() {
            let dropped = panel.strokes.len() - index - 1;
            if dropped > 0 {
                let noun = if dropped == 1 { "stroke" } else { "strokes" };
                warnings.push(Warning {
                    field: field_prefix.into(),
                    message: format!(
                        "{dropped} trailing {noun} dropped to stay within the panel-wide ink limit"
                    ),
                });
            }
            break;
        }
    }
}

/// How many leading points of `points` fit in the remaining ink budget; spends what it keeps.
/// Never invents or moves a point: the drawn stroke is always a prefix of the input.
fn ink_prefix_len(points: &[[f32; 2]], width: f64, remaining_ink: &mut f64) -> usize {
    // Each iteration consumes one segment of the finite slice, so this terminates.
    for (segment, pair) in points.windows(2).enumerate() {
        let dx = f64::from(pair[1][0] - pair[0][0]);
        let dy = f64::from(pair[1][1] - pair[0][1]);
        let ink = (dx * dx + dy * dy).sqrt() * width.max(1.0);
        if ink > *remaining_ink {
            *remaining_ink = 0.0;
            return segment + 1;
        }
        *remaining_ink -= ink;
    }
    points.len()
}

fn raster_stroke_points(
    pixmap: &mut Pixmap,
    points: &[[f32; 2]],
    paint: &Paint,
    style: &SkiaStroke,
) -> bool {
    let mut start = 0;
    // Every non-final iteration advances `start` by at least 510 while retaining one shared
    // boundary point, so a finite point slice produces finitely many raster chunks.
    while start + 1 < points.len() {
        let end = (start + MAX_STROKE_PATH_CHUNK_POINTS).min(points.len());
        let chunk = &points[start..end];
        let mut builder = PathBuilder::new();
        builder.move_to(chunk[0][0], chunk[0][1]);
        // This iterator consumes exactly one point from the finite chunk per iteration.
        for point in &chunk[1..] {
            builder.line_to(point[0], point[1]);
        }
        let Some(path) = builder.finish() else {
            return false;
        };
        pixmap.stroke_path(&path, paint, style, Transform::identity(), None);
        if end == points.len() {
            return true;
        }
        start = end - 1;
    }
    true
}

fn stroke_point_budget(width: u32, height: u32) -> usize {
    usize::try_from(width)
        .unwrap_or(usize::MAX)
        .saturating_add(usize::try_from(height).unwrap_or(usize::MAX))
        .saturating_mul(STROKE_POINTS_PER_OUTPUT_EDGE_PIXEL)
        .max(MIN_STROKE_PATH_POINTS)
}

/// Drops samples that cannot move the visible line by half an output pixel.
fn simplify_stroke_points(
    points: &[[f64; 2]],
    output_scale: f32,
    draw_scale: f32,
    draw_offset: f32,
) -> Vec<[f32; 2]> {
    let mut kept = Vec::with_capacity(points.len());
    let mut previous_output = None;
    // Each iteration consumes one input point and never revisits it, so simplification terminates.
    for point in points {
        let output = [
            point[0] as f32 * output_scale,
            point[1] as f32 * output_scale,
        ];
        if previous_output.is_none_or(|previous: [f32; 2]| {
            let dx = output[0] - previous[0];
            let dy = output[1] - previous[1];
            dx.mul_add(dx, dy * dy) >= 0.25
        }) {
            kept.push([
                point[0] as f32 * draw_scale + draw_offset,
                point[1] as f32 * draw_scale + draw_offset,
            ]);
            previous_output = Some(output);
        }
    }
    kept
}
fn empty_placeholder(pixmap: &mut Pixmap, width: u32, height: u32) {
    let rect = Rect::from_xywh(
        0.5,
        0.5,
        width.saturating_sub(1) as f32,
        height.saturating_sub(1) as f32,
    )
    .expect("positive canvas");
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(130, 125, 115, 255));
    let stroke = SkiaStroke {
        width: 1.0,
        ..SkiaStroke::default()
    };
    stroke_rectangle(pixmap, rect, &paint, &stroke);
    let mut path = PathBuilder::new();
    path.move_to(0.0, 0.0);
    path.line_to(width as f32, height as f32);
    path.move_to(width as f32, 0.0);
    path.line_to(0.0, height as f32);
    if let Some(path) = path.finish() {
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}
fn stroke_rectangle(pixmap: &mut Pixmap, rect: Rect, paint: &Paint, stroke: &SkiaStroke) {
    let path = PathBuilder::from_rect(rect);
    pixmap.stroke_path(&path, paint, stroke, Transform::identity(), None);
}
#[allow(clippy::too_many_arguments)]
fn draw_caption(
    pixmap: &mut Pixmap,
    project: &Project,
    context: PanelContext<'_>,
    image_height: u32,
    caption_height: u32,
    width: u32,
    scale: f32,
    warnings: &mut Vec<Warning>,
) {
    fill_rect(
        pixmap,
        Rect::from_xywh(
            0.0,
            image_height as f32,
            width as f32,
            caption_height as f32,
        )
        .expect("caption has size"),
        Color::from_rgba8(250, 248, 242, 255),
    );
    let (start, end) = story_frame_range(project, &context.shot.id);
    let mut text = format!(
        "{}  {}  {:.3}s  {}–{}f",
        context.number, context.shot.action, context.shot.duration, start, end
    );
    if !context.shot.dialogue.is_empty() {
        text.push_str(&format!("\n{}", context.shot.dialogue));
    }
    if !context.panel.direction.is_empty() {
        text.push_str(&format!("\n{}", context.panel.direction));
    }
    if !context.take.name.is_empty() {
        text.push_str(&format!("  [{}]", context.take.name));
    }
    let size = (22.0 * scale).clamp(10.0, 32.0);
    if text_overflows(
        &text,
        width as f32 - 12.0,
        caption_height as f32 - 8.0,
        size,
        Some("left"),
        Some("regular"),
    ) {
        warnings.push(Warning {
            field: format!("{}.caption.overflow", context.document_path),
            message: "caption text overflows its strip".into(),
        });
    }
    draw_text(
        pixmap,
        &text,
        6.0,
        image_height as f32 + 4.0,
        width as f32 - 12.0,
        caption_height as f32 - 8.0,
        size,
        "left",
        false,
        [30, 30, 30, 255],
    );
}
const MAX_LAYOUT_CHARS: usize = 512;

fn bound_text(text: &str, field: &str, warnings: &mut Vec<Warning>) -> String {
    if text.chars().count() <= MAX_LAYOUT_CHARS {
        return text.into();
    }
    warnings.push(Warning {
        field: format!("{field}.text"),
        message: format!("text layout was limited to {MAX_LAYOUT_CHARS} characters"),
    });
    text.chars().take(MAX_LAYOUT_CHARS).collect()
}

fn text_overflows(
    text: &str,
    width: f32,
    height: f32,
    size: f32,
    align: Option<&str>,
    weight: Option<&str>,
) -> bool {
    if width <= 0.0 || height <= 0.0 {
        return !text.is_empty();
    }
    let line_count = TEXT.with(|renderer| {
        renderer.borrow_mut().line_count(
            text,
            width,
            size,
            align.unwrap_or("left"),
            weight.unwrap_or("regular") == "bold",
        )
    });
    line_count as f32 * size * 1.2 > height
}
fn rgba(color: Color, opacity: f64) -> [u8; 4] {
    [
        (color.red() * 255.0) as u8,
        (color.green() * 255.0) as u8,
        (color.blue() * 255.0) as u8,
        (color.alpha() * opacity.clamp(0.0, 1.0) as f32 * 255.0) as u8,
    ]
}

struct TextRenderer {
    fonts: FontSystem,
    cache: SwashCache,
}
thread_local! { static TEXT: RefCell<TextRenderer> = RefCell::new(TextRenderer::new()); }
impl TextRenderer {
    fn new() -> Self {
        let mut database = fontdb::Database::new();
        database.load_font_data(FONT.to_vec());
        Self {
            fonts: FontSystem::new_with_locale_and_db("en-US".into(), database),
            cache: SwashCache::new(),
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn draw(
        &mut self,
        pixmap: &mut Pixmap,
        text: &str,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        size: f32,
        align: &str,
        bold: bool,
        color: [u8; 4],
    ) {
        let mut buffer = Buffer::new(
            &mut self.fonts,
            Metrics::new(size.max(1.0), size.max(1.0) * 1.2),
        );
        buffer.set_size(Some(width.max(0.0)), Some(height.max(0.0)));
        let attrs = Attrs::new()
            .family(Family::Name("Noto Sans"))
            .weight(if bold { Weight::BOLD } else { Weight::NORMAL });
        buffer.set_text(
            text,
            &attrs,
            Shaping::Advanced,
            Some(match align {
                "center" => Align::Center,
                "right" => Align::Right,
                _ => Align::Left,
            }),
        );
        let (canvas_width, canvas_height) = (pixmap.width() as i64, pixmap.height() as i64);
        let pixels = pixmap.data_mut();
        buffer.draw(
            &mut self.fonts,
            &mut self.cache,
            TextColor::rgba(color[0], color[1], color[2], color[3]),
            |gx, gy, gw, gh, glyph| {
                let [red, green, blue, alpha] = glyph.as_rgba();
                let left = (x.floor() as i64 + i64::from(gx)).max(0);
                let top = (y.floor() as i64 + i64::from(gy)).max(0);
                let right = (x.floor() as i64 + i64::from(gx) + i64::from(gw)).min(canvas_width);
                let bottom = (y.floor() as i64 + i64::from(gy) + i64::from(gh)).min(canvas_height);
                if left >= right || top >= bottom {
                    return;
                }
                for py in top..bottom {
                    for px in left..right {
                        let index = ((py * canvas_width + px) * 4) as usize;
                        let Some(pixel) = pixels.get_mut(index..index + 4) else {
                            continue;
                        };
                        let coverage = alpha as f32 / 255.0;
                        let inverse = 1.0 - coverage;
                        pixel[0] = (red as f32 * coverage + pixel[0] as f32 * inverse) as u8;
                        pixel[1] = (green as f32 * coverage + pixel[1] as f32 * inverse) as u8;
                        pixel[2] = (blue as f32 * coverage + pixel[2] as f32 * inverse) as u8;
                        pixel[3] = (255.0 * coverage + pixel[3] as f32 * inverse) as u8;
                    }
                }
            },
        );
    }

    fn line_count(&mut self, text: &str, width: f32, size: f32, align: &str, bold: bool) -> usize {
        let mut buffer = Buffer::new(
            &mut self.fonts,
            Metrics::new(size.max(1.0), size.max(1.0) * 1.2),
        );
        buffer.set_size(Some(width.max(0.0)), None);
        let attrs = Attrs::new()
            .family(Family::Name("Noto Sans"))
            .weight(if bold { Weight::BOLD } else { Weight::NORMAL });
        buffer.set_text(
            text,
            &attrs,
            Shaping::Advanced,
            Some(match align {
                "center" => Align::Center,
                "right" => Align::Right,
                _ => Align::Left,
            }),
        );
        buffer.shape_until_scroll(&mut self.fonts, false);
        buffer.layout_runs().count()
    }
}
#[allow(clippy::too_many_arguments)]
fn draw_text(
    pixmap: &mut Pixmap,
    text: &str,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    size: f32,
    align: &str,
    bold: bool,
    color: [u8; 4],
) {
    TEXT.with(|renderer| {
        renderer
            .borrow_mut()
            .draw(pixmap, text, x, y, width, height, size, align, bold, color)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Project {
        serde_json::from_str(include_str!("../../tests/fixtures/small.json")).expect("fixture")
    }

    #[test]
    fn canvas_limit_scales_panel_and_cut_sheet_before_allocating() {
        let mut project = project();
        project.format.width = 8192;
        project.format.height = 8192;
        let (width, warning) =
            fit_panel_width(&project, 8192, RenderMode::Stage).expect("panel layout");
        assert!(width < 8192);
        assert!(
            warning
                .expect("panel warning")
                .contains("requested 8192×8192")
        );

        let (width, warning) = fit_cut_sheet_width(&project, 10_000, 8192).expect("sheet layout");
        let layout = cut_sheet_layout(&project, 10_000, width).expect("scaled layout");
        assert!(width < 8192);
        assert!(u64::from(width) * u64::from(layout.height) <= MAX_CANVAS_PIXELS);
        assert!(warning.expect("cut-sheet warning").contains("requested"));
    }
}
