use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

use storyboard_core::{
    Layer, Project, RenderMode, Stroke, compile_story, describe_panel, describe_project,
    describe_sequence, panel_source_paths, render_cut_sheet, render_panel, validate,
};

const SMALL: &str = include_str!("fixtures/small.json");

fn fixture() -> Project {
    serde_json::from_str(SMALL).expect("fixture is a project")
}

fn panel(project: &mut Project, shot: usize) -> &mut storyboard_core::Panel {
    &mut project.scenes[if shot < 2 { 0 } else { 1 }].shots[shot % 2].takes[0].panels[0]
}

fn rgba(rendered: &storyboard_core::Rendered) -> image::RgbaImage {
    image::load_from_memory(&rendered.png)
        .expect("PNG decodes")
        .to_rgba8()
}

#[test]
fn describe_panel_names_its_story_context() {
    let project = fixture();
    let description = describe_panel(&project, "shot-1", "take-1", "panel-1").unwrap();
    assert!(description.contains("Arrival"));
    assert!(description.contains("Door"));
    assert!(description.contains("Door opens"));
}

#[test]
fn describe_panel_includes_direction() {
    let mut project = fixture();
    panel(&mut project, 0).direction = "Push through the doorway.".into();
    let description = describe_panel(&project, "shot-1", "take-1", "panel-1").unwrap();
    assert!(description.contains("Push through the doorway."));
}

#[test]
fn describe_project_and_sequence_are_readable() {
    let project = fixture();
    assert!(describe_project(&project).contains("4 shots"));
    let compiled = compile_story(&project).unwrap();
    let description = describe_sequence(&compiled);
    assert!(description.contains("shot-1"));
    assert!(description.contains("frames"));
}

#[test]
fn render_empty_panel_is_visible() {
    let mut project = fixture();
    let panel = panel(&mut project, 0);
    panel.layers.clear();
    panel.strokes.clear();
    panel.background = "#123456".into();
    let rendered = render_panel(
        &project,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        240,
        RenderMode::Stage,
    )
    .unwrap();
    let pixels = rgba(&rendered);
    assert_ne!(
        pixels.get_pixel(120, 75).0[..3],
        [18, 52, 86],
        "the placeholder must paint over an otherwise uniform panel"
    );
}

#[test]
fn render_missing_source_keeps_the_rest() {
    let mut project = fixture();
    let panel = panel(&mut project, 0);
    panel.layers.push(Layer {
        id: "missing-image".into(),
        layer_type: "image".into(),
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 100.0,
        opacity: 1.0,
        hidden: false,
        label: "Missing source".into(),
        src: Some("sources/nope.png".into()),
        fit: Some("contain".into()),
        crop: None,
        text: None,
        font_size: None,
        weight: None,
        align: None,
        color: None,
        fill: None,
        extra: Default::default(),
    });
    let rendered = render_panel(
        &project,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        240,
        RenderMode::Stage,
    )
    .unwrap();
    assert!(!rendered.warnings.is_empty());
    assert_ne!(rgba(&rendered).get_pixel(230, 130).0[..3], [0, 0, 0]);
}

#[test]
fn render_bold_differs_from_regular() {
    let mut regular = fixture();
    let text = Layer {
        id: "text".into(),
        layer_type: "text".into(),
        x: 100.0,
        y: 100.0,
        width: 1000.0,
        height: 300.0,
        opacity: 1.0,
        hidden: false,
        label: String::new(),
        src: None,
        fit: None,
        crop: None,
        text: Some("Bold?".into()),
        font_size: Some(120.0),
        weight: Some("regular".into()),
        align: Some("left".into()),
        color: Some("white".into()),
        fill: None,
        extra: Default::default(),
    };
    panel(&mut regular, 0).layers = vec![text.clone()];
    let mut bold = regular.clone();
    panel(&mut bold, 0).layers[0].weight = Some("bold".into());
    let regular = render_panel(
        &regular,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        240,
        RenderMode::Stage,
    )
    .unwrap();
    let bold = render_panel(
        &bold,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        240,
        RenderMode::Stage,
    )
    .unwrap();
    assert_ne!(regular.png, bold.png);
}

#[test]
fn render_strokes_draw() {
    let mut without = fixture();
    panel(&mut without, 0).layers.clear();
    panel(&mut without, 0).strokes.clear();
    let mut with = without.clone();
    panel(&mut with, 0).strokes.push(Stroke {
        color: "white".into(),
        width: 20.0,
        points: vec![[0.0, 0.0], [1440.0, 900.0]],
        extra: Default::default(),
    });
    let without = render_panel(
        &without,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        240,
        RenderMode::Stage,
    )
    .unwrap();
    let with = render_panel(
        &with,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        240,
        RenderMode::Stage,
    )
    .unwrap();
    assert_ne!(without.png, with.png);
}

#[test]
fn render_board_has_caption_strip() {
    let mut project = fixture();
    panel(&mut project, 0).direction = "Camera tracks left.".into();
    let stage = render_panel(
        &project,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        240,
        RenderMode::Stage,
    )
    .unwrap();
    let board = render_panel(
        &project,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        240,
        RenderMode::Board,
    )
    .unwrap();
    assert!(board.height > stage.height);
    assert_ne!(board.png, stage.png);
}

#[test]
fn render_cut_sheet_shows_each_shot() {
    let mut project = fixture();
    for (index, color) in ["red", "green", "blue", "yellow"].iter().enumerate() {
        let panel = panel(&mut project, index);
        panel.background = (*color).into();
        panel.layers.clear();
        panel.strokes.clear();
    }
    let compiled = compile_story(&project).unwrap();
    let rendered = render_cut_sheet(&project, Path::new("."), &compiled, 480).unwrap();
    let pixels = rgba(&rendered);
    let mut colors = std::collections::BTreeSet::new();
    for (_, _, pixel) in pixels.enumerate_pixels() {
        colors.insert([pixel[0], pixel[1], pixel[2]]);
    }
    assert!(colors.len() > 4, "contact sheet should retain its panels");
}

#[test]
fn render_warns_on_overflow() {
    let mut project = fixture();
    panel(&mut project, 0).layers = vec![Layer {
        id: "overflow".into(),
        layer_type: "text".into(),
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 10.0,
        opacity: 1.0,
        hidden: false,
        label: String::new(),
        src: None,
        fit: None,
        crop: None,
        text: Some("This text cannot fit in a ten-pixel box.".repeat(10)),
        font_size: Some(64.0),
        weight: Some("regular".into()),
        align: Some("left".into()),
        color: Some("white".into()),
        fill: None,
        extra: Default::default(),
    }];
    let rendered = render_panel(
        &project,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        240,
        RenderMode::Stage,
    )
    .unwrap();
    assert!(
        rendered
            .warnings
            .iter()
            .any(|warning| warning.field.contains("overflow"))
    );
}

#[test]
fn render_fit_fill_downsamples_thin_grid() {
    let root = tempfile::tempdir().expect("temporary project root");
    fs::create_dir(root.path().join("sources")).expect("sources directory");
    let mut grid = image::RgbaImage::from_pixel(1920, 1080, image::Rgba([255, 255, 255, 255]));
    for x in (0..1920).step_by(12) {
        for y in 0..1080 {
            grid.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
        }
    }
    for y in (0..1080).step_by(12) {
        for x in 0..1920 {
            grid.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
        }
    }
    grid.save(root.path().join("sources/grid.png"))
        .expect("grid PNG");

    let mut project = fixture();
    project.format.width = 1920;
    project.format.height = 1080;
    panel(&mut project, 0).layers = vec![Layer {
        id: "grid".into(),
        layer_type: "image".into(),
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
        opacity: 1.0,
        hidden: false,
        label: String::new(),
        src: Some("sources/grid.png".into()),
        fit: Some("fill".into()),
        crop: None,
        text: None,
        font_size: None,
        weight: None,
        align: None,
        color: None,
        fill: None,
        extra: Default::default(),
    }];
    let pixels = rgba(
        &render_panel(
            &project,
            root.path(),
            "shot-1",
            "take-1",
            "panel-1",
            320,
            RenderMode::Stage,
        )
        .expect("grid renders"),
    );

    for y in (0..pixels.height()).step_by(2) {
        assert!(
            pixels.get_pixel(1, y)[0] < 245,
            "horizontal grid line vanished at output row {y}"
        );
    }
    for x in (0..pixels.width()).step_by(2) {
        assert!(
            pixels.get_pixel(x, 1)[0] < 245,
            "vertical grid line vanished at output column {x}"
        );
    }
}

#[test]
fn render_fit_fill_extreme_upscale_keeps_raster_colour() {
    let root = tempfile::tempdir().expect("temporary project root");
    fs::create_dir(root.path().join("sources")).expect("sources directory");
    image::RgbaImage::from_pixel(20, 10, image::Rgba([255, 0, 0, 255]))
        .save(root.path().join("sources/red.png"))
        .expect("red PNG");

    let image = Layer {
        id: "red".into(),
        layer_type: "image".into(),
        x: -998_560.0,
        y: -999_100.0,
        width: 1_000_000.0,
        height: 1_000_000.0,
        opacity: 1.0,
        hidden: false,
        label: String::new(),
        src: Some("sources/red.png".into()),
        fit: Some("fill".into()),
        crop: None,
        text: None,
        font_size: None,
        weight: None,
        align: None,
        color: None,
        fill: None,
        extra: Default::default(),
    };

    let mut extreme = fixture();
    panel(&mut extreme, 0).background = "black".into();
    panel(&mut extreme, 0).layers = vec![image.clone()];
    panel(&mut extreme, 0).strokes.clear();
    let extreme = rgba(
        &render_panel(
            &extreme,
            root.path(),
            "shot-1",
            "take-1",
            "panel-1",
            8192,
            RenderMode::Stage,
        )
        .expect("extreme raster upscale renders"),
    );
    assert_eq!(
        extreme.get_pixel(4096, 2560).0,
        [255, 0, 0, 255],
        "a visible fraction of a raster source must retain its colour"
    );

    let mut ordinary = fixture();
    ordinary.format.width = 20;
    ordinary.format.height = 10;
    let mut ordinary_image = image;
    ordinary_image.x = 0.0;
    ordinary_image.y = 0.0;
    ordinary_image.width = 20.0;
    ordinary_image.height = 10.0;
    panel(&mut ordinary, 0).layers = vec![ordinary_image];
    panel(&mut ordinary, 0).strokes.clear();
    for (width, x, y) in [(20, 10, 5), (40, 20, 10)] {
        let rendered = rgba(
            &render_panel(
                &ordinary,
                root.path(),
                "shot-1",
                "take-1",
                "panel-1",
                width,
                RenderMode::Stage,
            )
            .expect("ordinary raster scale renders"),
        );
        assert_eq!(rendered.get_pixel(x, y).0, [255, 0, 0, 255]);
    }
}

#[test]
fn render_circle_stroke_keeps_its_shape_and_second_stroke() {
    let mut project = fixture();
    let panel = panel(&mut project, 0);
    panel.background = "black".into();
    panel.layers.clear();
    let centre = [720.0, 450.0];
    let radius = 200.0;
    let circle = (0..=200)
        .map(|point| {
            let angle = point as f64 * std::f64::consts::TAU / 200.0;
            [
                centre[0] + radius * angle.cos(),
                centre[1] + radius * angle.sin(),
            ]
        })
        .collect();
    panel.strokes = vec![
        Stroke {
            color: "white".into(),
            width: 30.0,
            points: circle,
            extra: Default::default(),
        },
        Stroke {
            color: "red".into(),
            width: 30.0,
            points: vec![[100.0, 700.0], [400.0, 700.0]],
            extra: Default::default(),
        },
    ];

    let pixels = rgba(
        &render_panel(
            &project,
            Path::new("."),
            "shot-1",
            "take-1",
            "panel-1",
            320,
            RenderMode::Stage,
        )
        .expect("circle and second stroke render"),
    );
    let scale = 320.0 / 1440.0;
    for degrees in (0..360).step_by(45) {
        let angle = f64::from(degrees).to_radians();
        let x = ((centre[0] + radius * angle.cos()) * scale).round() as u32;
        let y = ((centre[1] + radius * angle.sin()) * scale).round() as u32;
        assert_eq!(
            pixels.get_pixel(x, y).0[..3],
            [255, 255, 255],
            "circle ink is missing at {degrees} degrees"
        );
    }
    assert_eq!(pixels.get_pixel(160, 100).0[..3], [0, 0, 0]);
    assert_eq!(pixels.get_pixel(56, 156).0[..3], [255, 0, 0]);
}

#[test]
fn render_extreme_text_is_bounded_and_warns() {
    let mut project = fixture();
    project.format.width = 2;
    project.format.height = 1;
    panel(&mut project, 0).layers = vec![Layer {
        id: "large-text".into(),
        layer_type: "text".into(),
        x: 0.0,
        y: 0.0,
        width: 2.0,
        height: 1.0,
        opacity: 1.0,
        hidden: false,
        label: String::new(),
        src: None,
        fit: None,
        crop: None,
        text: Some("M".repeat(20_000)),
        font_size: Some(4096.0),
        weight: Some("regular".into()),
        align: Some("left".into()),
        color: Some("black".into()),
        fill: None,
        extra: Default::default(),
    }];
    let started = Instant::now();
    let rendered = render_panel(
        &project,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        8192,
        RenderMode::Stage,
    )
    .expect("validated extreme text renders");
    assert!(started.elapsed() < Duration::from_secs(60));
    assert!(rendered.warnings.iter().any(|warning| {
        warning.message.contains("font size") || warning.message.contains("layout")
    }));
}

#[test]
fn render_board_caption_uses_shot_timing_not_panel_offset() {
    let mut project = fixture();
    let mut later = panel(&mut project, 0).clone();
    later.id = "panel-later".into();
    later.time = 1.0;
    project.scenes[0].shots[0].takes[0].panels.push(later);
    let first = render_panel(
        &project,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        320,
        RenderMode::Board,
    )
    .expect("first board");
    let second = render_panel(
        &project,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-later",
        320,
        RenderMode::Board,
    )
    .expect("later board");
    assert_eq!(
        first.png, second.png,
        "caption should not show panel offset"
    );
}

#[test]
fn render_cut_sheet_has_gutters_and_distinct_tiles() {
    let mut project = fixture();
    for (index, color) in ["red", "green", "blue", "yellow"].iter().enumerate() {
        let panel = panel(&mut project, index);
        panel.background = (*color).into();
        panel.layers.clear();
        panel.strokes.clear();
    }
    let compiled = compile_story(&project).expect("compiled sequence");
    let rendered = render_cut_sheet(&project, Path::new("."), &compiled, 480).expect("sheet");
    let pixels = rgba(&rendered);
    let gutter = pixels.get_pixel(160, 4).0;
    assert_eq!(gutter[..3], [232, 229, 222], "tiles need visible gutters");
    for (x, expected) in [(40, [255, 0, 0]), (200, [0, 128, 0]), (360, [0, 0, 255])] {
        let pixel = pixels.get_pixel(x, 40).0;
        assert_eq!(pixel[..3], expected, "tile at x={x} lost its shot color");
    }
}

#[test]
fn describe_panel_includes_rendering_context_and_sources() {
    let mut project = fixture();
    let shot = &mut project.scenes[0].shots[0];
    shot.speaker = "Ari".into();
    shot.camera = "wide".into();
    shot.action = "Open the door".into();
    shot.dialogue = "Are you there?".into();
    shot.refs = vec!["sources/shot-ref.png".into()];
    shot.takes[0].refs = vec!["sources/take-ref.png".into()];
    let panel = &mut shot.takes[0].panels[0];
    panel.direction = "Push in".into();
    panel.layers[0].label = "Backdrop".into();
    let description = describe_panel(&project, "shot-1", "take-1", "panel-1").expect("description");
    for expected in [
        "Duration: 2.000s",
        "Speaker: Ari",
        "Camera: wide",
        "Action:",
        "Dialogue:",
        "Direction: Push in",
        "Layers (1)",
        "solid",
        "Strokes: 1",
        "sources/shot-ref.png",
        "sources/take-ref.png",
    ] {
        assert!(
            description.contains(expected),
            "missing {expected:?}: {description}"
        );
    }
}

#[test]
fn render_svg_source_at_target_size() {
    let root = tempfile::tempdir().expect("temporary project root");
    fs::create_dir(root.path().join("sources")).expect("sources directory");
    fs::write(
        root.path().join("sources/red.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="red"/></svg>"#,
    )
    .expect("SVG source");
    let mut project = fixture();
    panel(&mut project, 0).layers = vec![Layer {
        id: "svg".into(),
        layer_type: "image".into(),
        x: 0.0,
        y: 0.0,
        width: 1440.0,
        height: 900.0,
        opacity: 1.0,
        hidden: false,
        label: String::new(),
        src: Some("sources/red.svg".into()),
        fit: Some("fill".into()),
        crop: None,
        text: None,
        font_size: None,
        weight: None,
        align: None,
        color: None,
        fill: None,
        extra: Default::default(),
    }];
    let rendered = render_panel(
        &project,
        root.path(),
        "shot-1",
        "take-1",
        "panel-1",
        320,
        RenderMode::Stage,
    )
    .expect("SVG renders");
    assert_eq!(rgba(&rendered).get_pixel(160, 100).0[..3], [255, 0, 0]);
}

#[test]
fn render_metadata_and_errors_serialize_without_png_bytes() {
    let rendered = storyboard_core::Rendered {
        png: vec![1, 2, 3],
        width: 2,
        height: 1,
        warnings: vec![storyboard_core::Warning {
            field: "project.title".into(),
            message: "note".into(),
        }],
    };
    let value = serde_json::to_value(rendered).expect("render metadata serializes");
    assert!(value.get("png").is_none());
    assert_eq!(
        serde_json::to_value(RenderMode::Board).expect("mode serializes"),
        "Board"
    );
    let error = render_panel(
        &fixture(),
        Path::new("."),
        "missing",
        "take",
        "panel",
        320,
        RenderMode::Stage,
    )
    .expect_err("unknown shot fails");
    assert_eq!(error.kind, "shot");
    assert!(!error.hint.is_empty());
    assert!(error.valid.is_empty());
}

#[test]
fn panel_source_paths_and_warning_fields_are_document_paths() {
    let mut project = fixture();
    let mut image = panel(&mut project, 0).layers[0].clone();
    image.id = "source".into();
    image.layer_type = "image".into();
    image.src = Some("sources/missing.png".into());
    image.fit = Some("fill".into());
    panel(&mut project, 0).layers.push(image);
    assert_eq!(
        panel_source_paths(&project, "shot-1", "take-1", "panel-1").expect("source paths"),
        vec!["sources/missing.png"]
    );
    let rendered = render_panel(
        &project,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        320,
        RenderMode::Stage,
    )
    .expect("missing source is a warning");
    assert!(
        rendered.warnings.iter().any(|warning| {
            warning.field == "scenes[0].shots[0].takes[0].panels[0].layers[1].src"
        })
    );
}

#[test]
fn render_extremes_allowed_by_validation_terminate_without_panicking() {
    let mut project = fixture();
    let panel = panel(&mut project, 0);
    panel.layers = (0..500)
        .map(|index| Layer {
            id: format!("off-canvas-{index}"),
            layer_type: "box".into(),
            x: 1_000_000.0,
            y: 1_000_000.0,
            width: 1.0,
            height: 1.0,
            opacity: 1.0,
            hidden: false,
            label: String::new(),
            src: None,
            fit: None,
            crop: None,
            text: None,
            font_size: None,
            weight: None,
            align: None,
            color: None,
            fill: None,
            extra: Default::default(),
        })
        .collect();
    panel.layers.push(Layer {
        id: "long-text".into(),
        layer_type: "text".into(),
        x: 0.0,
        y: 0.0,
        width: 1440.0,
        height: 900.0,
        opacity: 1.0,
        hidden: false,
        label: String::new(),
        src: None,
        fit: None,
        crop: None,
        text: Some("x".repeat(20_000)),
        font_size: Some(4096.0),
        weight: Some("regular".into()),
        align: Some("left".into()),
        color: Some("black".into()),
        fill: None,
        extra: Default::default(),
    });
    panel.strokes.push(Stroke {
        color: "black".into(),
        width: 1_000_000.0,
        points: (0..10_000)
            .map(|point| [point as f64, point as f64])
            .collect(),
        extra: Default::default(),
    });
    assert!(validate(&project).is_empty(), "extreme fixture stays valid");
    let started = Instant::now();
    let result = render_panel(
        &project,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        320,
        RenderMode::Stage,
    );
    assert!(
        result.is_ok(),
        "validated limits render with warnings: {result:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn render_huge_svg_raster_work_is_clipped_to_the_canvas() {
    let root = tempfile::tempdir().expect("temporary project root");
    fs::create_dir(root.path().join("sources")).expect("sources directory");
    fs::write(
        root.path().join("sources/large.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="red"/></svg>"#,
    )
    .expect("SVG source");
    let mut project = fixture();
    panel(&mut project, 0).layers = vec![Layer {
        id: "large-svg".into(),
        layer_type: "image".into(),
        x: -998_560.0,
        y: -999_100.0,
        width: 1_000_000.0,
        height: 1_000_000.0,
        opacity: 1.0,
        hidden: false,
        label: String::new(),
        src: Some("sources/large.svg".into()),
        fit: Some("fill".into()),
        crop: None,
        text: None,
        font_size: None,
        weight: None,
        align: None,
        color: None,
        fill: None,
        extra: Default::default(),
    }];

    let rendered = render_panel(
        &project,
        root.path(),
        "shot-1",
        "take-1",
        "panel-1",
        320,
        RenderMode::Stage,
    )
    .expect("huge clipped SVG renders");
    assert_eq!(
        storyboard_core::render::last_image_raster_target_size(),
        Some((320, 200)),
        "SVG raster work must be limited to its visible canvas intersection"
    );
    assert!(
        rendered
            .warnings
            .iter()
            .any(|warning| warning.message.contains("clipped to the canvas"))
    );

    image::RgbaImage::from_pixel(20, 10, image::Rgba([255, 0, 0, 255]))
        .save(root.path().join("sources/large.png"))
        .expect("raster source");
    panel(&mut project, 0).layers[0].src = Some("sources/large.png".into());
    let rendered = render_panel(
        &project,
        root.path(),
        "shot-1",
        "take-1",
        "panel-1",
        320,
        RenderMode::Stage,
    )
    .expect("huge clipped raster renders");
    assert_eq!(
        storyboard_core::render::last_image_raster_target_size(),
        Some((320, 200)),
        "raster work must be limited to its visible canvas intersection"
    );
    assert!(
        rendered
            .warnings
            .iter()
            .any(|warning| warning.message.contains("clipped to the canvas"))
    );
}

#[test]
fn render_scattered_stroke_points_is_bounded() {
    let mut project = fixture();
    let panel = panel(&mut project, 0);
    panel.layers.clear();
    panel.strokes = vec![Stroke {
        color: "white".into(),
        width: 3.0,
        points: (0_usize..100_000)
            .map(|point| {
                [
                    (point.wrapping_mul(977) % 1440) as f64,
                    (point.wrapping_mul(619) % 900) as f64,
                ]
            })
            .collect(),
        extra: Default::default(),
    }];

    let started = Instant::now();
    let rendered = render_panel(
        &project,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        8192,
        RenderMode::Stage,
    )
    .expect("dense stroke renders");
    assert!(started.elapsed() < Duration::from_secs(20));
    assert!(
        rendered
            .warnings
            .iter()
            .all(|warning| !warning.message.contains("stroke points were simplified")),
        "a 100k-point stroke is below the output-scaled cap at 8192px"
    );
    assert!(
        rendered
            .warnings
            .iter()
            .any(|warning| warning.message.contains("panel-wide ink limit")),
        "a canvas-crossing zigzag this long is cut at the ink limit, with a warning"
    );
    let pixels = rgba(&rendered);
    let (cell_width, cell_height) = (pixels.width() / 8, pixels.height() / 8);
    let mut inked_cells = 0;
    for cell_y in 0..8 {
        for cell_x in 0..8 {
            let inked = (0..cell_height).step_by(2).any(|dy| {
                (0..cell_width).step_by(2).any(|dx| {
                    pixels
                        .get_pixel(cell_x * cell_width + dx, cell_y * cell_height + dy)
                        .0[..3]
                        != [17, 17, 17]
                })
            });
            inked_cells += usize::from(inked);
        }
    }
    assert!(
        inked_cells >= 60,
        "the kept part of the scattered stroke spreads across the canvas, not a triangle: {inked_cells}/64 cells"
    );
}

#[test]
fn render_hatching_and_circle_finish_at_supported_widths() {
    let mut project = fixture();
    let panel = panel(&mut project, 0);
    panel.background = "black".into();
    panel.layers.clear();
    panel.strokes = (0..180)
        .map(|line| Stroke {
            color: "red".into(),
            width: 1.0,
            points: vec![[0.0, f64::from(line * 5)], [1440.0, f64::from(line * 5)]],
            extra: Default::default(),
        })
        .collect();
    let centre = [720.0, 450.0];
    let radius = 200.0;
    panel.strokes.push(Stroke {
        color: "white".into(),
        width: 12.0,
        points: (0..200)
            .map(|sample| {
                let angle = sample as f64 * std::f64::consts::TAU / 199.0;
                [
                    centre[0] + radius * angle.cos(),
                    centre[1] + radius * angle.sin(),
                ]
            })
            .collect(),
        extra: Default::default(),
    });

    for width in [320, 1440, 8192] {
        let started = Instant::now();
        let rendered = render_panel(
            &project,
            Path::new("."),
            "shot-1",
            "take-1",
            "panel-1",
            width,
            RenderMode::Stage,
        )
        .expect("hatching and circle render");
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "{width}px render exceeded the debug-time regression bound"
        );
        let pixels = rgba(&rendered);
        let scale = f64::from(width) / 1440.0;
        let first_hatch_y = (100.0 * scale).round() as u32;
        let second_hatch_y = (105.0 * scale).round() as u32;
        assert_ne!(first_hatch_y, second_hatch_y);
        for y in [first_hatch_y, second_hatch_y] {
            let pixel = pixels.get_pixel((40.0 * scale).round() as u32, y);
            assert!(
                pixel[0] > 100 && pixel[1] < 100,
                "hatch line is missing at output row {y} for {width}px"
            );
        }
        for sample in 0..72 {
            let angle = sample as f64 * std::f64::consts::TAU / 72.0;
            let x = ((centre[0] + radius * angle.cos()) * scale).round() as u32;
            let y = ((centre[1] + radius * angle.sin()) * scale).round() as u32;
            assert!(
                pixels.get_pixel(x, y)[1] > 100,
                "circle ink is missing at sample {sample} for {width}px"
            );
        }
    }
}

#[test]
fn render_stroke_never_invents_extent_anchor_points() {
    let mut project = fixture();
    let panel = panel(&mut project, 0);
    panel.background = "black".into();
    panel.layers.clear();
    let diamond = [
        [920.0, 450.0],
        [720.0, 650.0],
        [520.0, 450.0],
        [720.0, 250.0],
    ];
    panel.strokes = vec![Stroke {
        color: "white".into(),
        width: 12.0,
        points: (0..4_001).map(|index| diamond[index % 4]).collect(),
        extra: Default::default(),
    }];

    let pixels = rgba(
        &render_panel(
            &project,
            Path::new("."),
            "shot-1",
            "take-1",
            "panel-1",
            320,
            RenderMode::Stage,
        )
        .expect("repeated diamond renders"),
    );
    let output_diamond = [
        [204.444_f64, 100.0],
        [160.0, 144.444],
        [115.556, 100.0],
        [160.0, 55.556],
    ];
    for (x, y, pixel) in pixels.enumerate_pixels() {
        if pixel.0[..3] == [0, 0, 0] {
            continue;
        }
        let point = [f64::from(x) + 0.5, f64::from(y) + 0.5];
        let distance_squared = (0..4)
            .map(|index| {
                let start = output_diamond[index];
                let end = output_diamond[(index + 1) % 4];
                let delta = [end[0] - start[0], end[1] - start[1]];
                let length_squared = delta[0] * delta[0] + delta[1] * delta[1];
                let projection = (((point[0] - start[0]) * delta[0]
                    + (point[1] - start[1]) * delta[1])
                    / length_squared)
                    .clamp(0.0, 1.0);
                let closest = [
                    start[0] + projection * delta[0],
                    start[1] + projection * delta[1],
                ];
                (point[0] - closest[0]).powi(2) + (point[1] - closest[1]).powi(2)
            })
            .fold(f64::INFINITY, f64::min);
        assert!(
            distance_squared <= 9.0,
            "stroke preparation invented ink at ({x}, {y}), away from every input segment"
        );
    }
}

#[test]
fn render_over_cap_panel_drops_trailing_strokes_as_whole_strokes() {
    let mut earlier_only = fixture();
    let earlier_panel = panel(&mut earlier_only, 0);
    earlier_panel.background = "black".into();
    earlier_panel.layers.clear();
    earlier_panel.strokes = vec![Stroke {
        color: "white".into(),
        width: 1440.0,
        points: (0..30_000)
            .map(|index| [if index % 2 == 0 { 0.0 } else { 1440.0 }, 0.0])
            .collect(),
        extra: Default::default(),
    }];
    let earlier = render_panel(
        &earlier_only,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        1,
        RenderMode::Stage,
    )
    .expect("earlier stroke renders");

    let mut over_cap = earlier_only.clone();
    panel(&mut over_cap, 0).strokes.push(Stroke {
        color: "red".into(),
        width: 1440.0,
        points: (0..30_000)
            .map(|index| [if index % 2 == 0 { 0.0 } else { 1440.0 }, 0.0])
            .collect(),
        extra: Default::default(),
    });
    let rendered = render_panel(
        &over_cap,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        1,
        RenderMode::Stage,
    )
    .expect("over-cap panel renders");

    assert_eq!(
        rendered.png, earlier.png,
        "the kept prefix must be byte-for-byte intact"
    );
    let drop_warnings = rendered
        .warnings
        .iter()
        .filter(|warning| warning.message.contains("dropped"))
        .collect::<Vec<_>>();
    assert_eq!(drop_warnings.len(), 1, "one panel-wide warning is emitted");
    assert!(drop_warnings[0].message.contains("1 trailing stroke"));
}

#[test]
fn render_large_text_is_not_hard_capped_below_canvas_height() {
    let mut project = fixture();
    project.format.width = 3840;
    project.format.height = 2160;
    panel(&mut project, 0).layers = vec![Layer {
        id: "large-text".into(),
        layer_type: "text".into(),
        x: 0.0,
        y: 0.0,
        width: 3840.0,
        height: 2160.0,
        opacity: 1.0,
        hidden: false,
        label: String::new(),
        src: None,
        fit: None,
        crop: None,
        text: Some("Large".into()),
        font_size: Some(512.0),
        weight: Some("regular".into()),
        align: Some("left".into()),
        color: Some("white".into()),
        fill: None,
        extra: Default::default(),
    }];

    let rendered = render_panel(
        &project,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        3840,
        RenderMode::Stage,
    )
    .expect("large text renders");
    assert!(
        !rendered
            .warnings
            .iter()
            .any(|warning| warning.field.ends_with("font_size")),
        "a normal 4K font size must not be clamped to 128px"
    );
}

#[test]
fn render_dense_panel_keeps_separate_lines() {
    let mut project = fixture();
    let panel = panel(&mut project, 0);
    panel.background = "black".into();
    panel.layers.clear();
    panel.strokes = (0..60)
        .map(|line| Stroke {
            color: "white".into(),
            width: 2.0,
            points: (0..1000)
                .map(|step| {
                    let x = step as f64 * 1440.0 / 999.0;
                    [x, 7.5 + line as f64 * 15.0 + 3.0 * (x / 40.0).sin()]
                })
                .collect(),
            extra: Default::default(),
        })
        .collect();

    let rendered = render_panel(
        &project,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        1440,
        RenderMode::Stage,
    )
    .expect("dense panel renders");
    let pixels = rgba(&rendered);
    let column = 720;
    let mut runs = 0;
    let mut inked = false;
    for y in 0..pixels.height() {
        let ink = pixels.get_pixel(column, y).0[0] > 128;
        if ink && !inked {
            runs += 1;
        }
        inked = ink;
    }
    assert!(
        runs >= 55,
        "60 dense strokes must stay separate lines, found {runs} ink runs"
    );
}

#[test]
fn render_dense_straight_line_stays_crisp() {
    let mut project = fixture();
    let panel = panel(&mut project, 0);
    panel.background = "black".into();
    panel.layers.clear();
    panel.strokes = vec![Stroke {
        color: "white".into(),
        width: 2.0,
        points: (0..50_000)
            .map(|step| {
                let t = step as f64 / 49_999.0;
                [t * 1440.0, t * 900.0]
            })
            .collect(),
        extra: Default::default(),
    }];

    let rendered = render_panel(
        &project,
        Path::new("."),
        "shot-1",
        "take-1",
        "panel-1",
        1440,
        RenderMode::Stage,
    )
    .expect("dense line renders");
    let pixels = rgba(&rendered);
    assert!(pixels.get_pixel(720, 450).0[0] > 128, "the line is drawn");
    for (x, y) in [(720, 420), (720, 480), (300, 600), (1100, 200)] {
        assert!(
            pixels.get_pixel(x, y).0[0] < 64,
            "pixel ({x}, {y}) is off the line and must stay background"
        );
    }
}
