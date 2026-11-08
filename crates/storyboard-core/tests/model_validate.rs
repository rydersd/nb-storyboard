use storyboard_core::{Project, validate};

const SMALL: &str = include_str!("fixtures/small.json");

fn fixture() -> Project {
    serde_json::from_str(SMALL).expect("the small fixture is a project")
}

#[test]
fn model_round_trips_fixture_exactly() {
    let project = fixture();
    assert_eq!(project.to_pretty_json().unwrap(), SMALL.trim_end());
}

#[test]
fn model_keeps_unknown_fields_everywhere() {
    let mut value: serde_json::Value = serde_json::from_str(SMALL).unwrap();
    value["project_future"] = serde_json::json!("project");
    value["format"]["format_future"] = serde_json::json!("format");
    value["scenes"][0]["scene_future"] = serde_json::json!("scene");
    value["scenes"][0]["shots"][0]["shot_future"] = serde_json::json!("shot");
    value["scenes"][0]["shots"][0]["takes"][0]["take_future"] = serde_json::json!("take");
    value["scenes"][0]["shots"][0]["takes"][0]["panels"][0]["panel_future"] =
        serde_json::json!("panel");
    value["scenes"][0]["shots"][0]["takes"][0]["panels"][0]["layers"][0]["layer_future"] =
        serde_json::json!("layer");
    value["cuts"][0]["cut_future"] = serde_json::json!("cut");
    value["cuts"][0]["clips"][0]["clip_future"] = serde_json::json!("clip");
    value["cuts"][0]["clips"][0]["source"]["shot"]["source_future"] = serde_json::json!("source");
    value["scenes"][0]["shots"][0]["takes"][0]["panels"][0]["strokes"][0]["stroke_future"] =
        serde_json::json!("stroke");
    value["stickies"][0]["sticky_future"] = serde_json::json!("sticky");
    value["layout"]["layout_future"] = serde_json::json!("layout");
    value["layout"]["positions"]["shot:shot-1"]["position_future"] = serde_json::json!("position");

    let project: Project = serde_json::from_value(value).unwrap();
    let written = serde_json::to_value(project).unwrap();
    for key in ["project_future", "scenes", "cuts", "stickies", "layout"] {
        assert!(written.get(key).is_some(), "missing {key}");
    }
    assert_eq!(written["project_future"], "project");
    assert_eq!(written["format"]["format_future"], "format");
    assert_eq!(written["scenes"][0]["scene_future"], "scene");
    assert_eq!(written["scenes"][0]["shots"][0]["shot_future"], "shot");
    assert_eq!(
        written["scenes"][0]["shots"][0]["takes"][0]["take_future"],
        "take"
    );
    assert_eq!(
        written["scenes"][0]["shots"][0]["takes"][0]["panels"][0]["panel_future"],
        "panel"
    );
    assert_eq!(
        written["scenes"][0]["shots"][0]["takes"][0]["panels"][0]["layers"][0]["layer_future"],
        "layer"
    );
    assert_eq!(written["cuts"][0]["cut_future"], "cut");
    assert_eq!(written["cuts"][0]["clips"][0]["clip_future"], "clip");
    assert_eq!(
        written["cuts"][0]["clips"][0]["source"]["shot"]["source_future"],
        "source"
    );
    assert_eq!(
        written["scenes"][0]["shots"][0]["takes"][0]["panels"][0]["strokes"][0]["stroke_future"],
        "stroke"
    );
    assert_eq!(written["stickies"][0]["sticky_future"], "sticky");
    assert_eq!(written["layout"]["layout_future"], "layout");
    assert_eq!(
        written["layout"]["positions"]["shot:shot-1"]["position_future"],
        "position"
    );
}

#[test]
fn model_writes_id_first_and_omits_empty() {
    let project = fixture();
    let written = project.to_pretty_json().unwrap();
    let value = serde_json::to_value(project).unwrap();
    let scene_keys: Vec<_> = value["scenes"][0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(scene_keys.first(), Some(&"id"), "scene id must be first");
    assert!(!written.contains("\"action\": \"\""));
    assert!(!written.contains("\"refs\": []"));
    assert!(!written.contains("\"layers\": []"));
}

#[test]
fn model_keeps_unknown_compaction_marker_key() {
    let mut project = fixture();
    project.extra.insert(
        "__nb_storyboard_compact_stroke_points__".into(),
        serde_json::json!([1, 2, 3]),
    );
    let written = project.to_pretty_json().unwrap();
    let value: serde_json::Value = serde_json::from_str(&written).unwrap();
    assert_eq!(
        value["__nb_storyboard_compact_stroke_points__"],
        serde_json::json!([1, 2, 3])
    );
    assert!(!value.get("points").is_some_and(serde_json::Value::is_array));
}

#[test]
fn model_preserves_large_unknown_numbers_in_tagged_objects() {
    let numbers = [
        "1234567890123456789",
        "12345678901234567890",
        "123456789012345678901234567890",
        "1234567890123456789012345678901234567890123",
        "123456789012345678901234567890.123456789012345678901234567890",
    ];
    for number in numbers {
        for (tag, replacement, direct_pointer, nested_pointer, outer_pointer) in [
            (
                "shot",
                format!(
                    "\"shot\": {{\"shot\": \"shot-1\", \"unknown\": {number}, \"nested\": {{\"unknown\": {number}}}}}, \"outer_unknown\": {number}"
                ),
                "/cuts/0/clips/0/source/shot/unknown",
                "/cuts/0/clips/0/source/shot/nested/unknown",
                "/cuts/0/clips/0/source/outer_unknown",
            ),
            (
                "cut",
                format!(
                    "\"cut\": {{\"cut\": \"cut-alt\", \"unknown\": {number}, \"nested\": {{\"unknown\": {number}}}}}, \"outer_unknown\": {number}"
                ),
                "/cuts/0/clips/1/source/cut/unknown",
                "/cuts/0/clips/1/source/cut/nested/unknown",
                "/cuts/0/clips/1/source/outer_unknown",
            ),
            (
                "agent",
                r#""author": {"agent": {"name": "agent", "unknown": NUMBER, "nested": {"unknown": NUMBER}}, "outer_unknown": NUMBER}"#
                    .replace("NUMBER", number),
                "/stickies/0/author/agent/unknown",
                "/stickies/0/author/agent/nested/unknown",
                "/stickies/0/author/outer_unknown",
            ),
        ] {
            let input = match tag {
                "shot" => SMALL.replacen(
                    "\"shot\": {\n              \"shot\": \"shot-1\"\n            }",
                    &replacement,
                    1,
                ),
                "cut" => SMALL.replacen(
                    "\"cut\": {\n              \"cut\": \"cut-alt\"\n            }",
                    &replacement,
                    1,
                ),
                "agent" => SMALL.replacen("\"author\": \"human\"", &replacement, 1),
                _ => unreachable!(),
            };
            let project: Project = serde_json::from_str(&input)
                .unwrap_or_else(|error| panic!("{tag} rejected {number}: {error}"));
            let written: serde_json::Value =
                serde_json::from_str(&project.to_pretty_json().unwrap()).unwrap();
            for pointer in [direct_pointer, nested_pointer, outer_pointer] {
                assert!(
                    written
                        .pointer(pointer)
                        .is_some_and(serde_json::Value::is_number),
                    "{tag} did not retain {number} at {pointer}: {written}"
                );
            }
        }
    }
}

#[test]
fn model_strings_round_trip_byte_identically_across_repeated_saves() {
    let input = SMALL.replace("\"Small\"", "\"Café — see [1, 2] and [3 4]\"");
    let first: Project = serde_json::from_str(&input).unwrap();
    let once = first.to_pretty_json().unwrap();
    let twice = serde_json::from_str::<Project>(&once)
        .unwrap()
        .to_pretty_json()
        .unwrap();
    assert_eq!(once, twice);
    assert!(once.contains("Café — see [1, 2] and [3 4]"));
}

#[test]
fn model_keeps_unknown_field_order_in_tagged_objects_across_saves() {
    let unknowns = r#""u1": 1, "u2": 2, "u3": 3, "u4": 4"#;
    let input = SMALL
        .replacen(
            r#""shot": {
              "shot": "shot-1"
            }"#,
            &format!(r#""shot": {{"shot": "shot-1", {unknowns}}}, {unknowns}"#),
            1,
        )
        .replacen(
            r#""cut": {
              "cut": "cut-alt"
            }"#,
            &format!(r#""cut": {{"cut": "cut-alt", {unknowns}}}, {unknowns}"#),
            1,
        )
        .replacen(
            r#""author": "human""#,
            &format!(r#""author": {{"agent": {{"name": "agent", {unknowns}}}, {unknowns}}}"#),
            1,
        );

    let first = serde_json::from_str::<Project>(&input)
        .unwrap()
        .to_pretty_json()
        .unwrap();
    let second = serde_json::from_str::<Project>(&first)
        .unwrap()
        .to_pretty_json()
        .unwrap();
    let third = serde_json::from_str::<Project>(&second)
        .unwrap()
        .to_pretty_json()
        .unwrap();
    assert_eq!(first, second, "the file must settle after the first save");
    assert_eq!(second, third, "later saves must remain byte-identical");

    let value: serde_json::Value = serde_json::from_str(&third).unwrap();
    for pointer in [
        "/cuts/0/clips/0/source",
        "/cuts/0/clips/0/source/shot",
        "/cuts/0/clips/1/source",
        "/cuts/0/clips/1/source/cut",
        "/stickies/0/author",
        "/stickies/0/author/agent",
    ] {
        let object = value.pointer(pointer).unwrap().as_object().unwrap();
        let unknown_keys: Vec<_> = object
            .keys()
            .filter(|key| key.starts_with('u'))
            .map(String::as_str)
            .collect();
        assert_eq!(unknown_keys, ["u1", "u2", "u3", "u4"], "{pointer}");
    }
}

#[test]
fn model_preserves_unknown_fields_in_order_with_exact_values() {
    let input = SMALL.replacen(
        "\"height\": 900\n  },",
        "\"height\": 900,\n    \"future_first\": 123456789012345678901234567890,\n    \"future_second\": {\"z\": 1, \"a\": 2}\n  },",
        1,
    )
    .replacen("\"x\": 0.0", "\"x\": 1.25", 1);
    let project: Project = serde_json::from_str(&input).unwrap();
    assert_eq!(
        project.scenes[0].shots[0].takes[0].panels[0].layers[0].x,
        1.25
    );
    let written = project.to_pretty_json().unwrap();
    let width = written.find("\"width\"").unwrap();
    let height = written.find("\"height\"").unwrap();
    let first = written.find("\"future_first\"").unwrap();
    let second = written.find("\"future_second\"").unwrap();
    assert!(width < height && height < first && first < second);
    assert!(written.contains("123456789012345678901234567890"));
    let written: serde_json::Value = serde_json::from_str(&written).unwrap();
    let nested = written["format"]["future_second"].as_object().unwrap();
    assert_eq!(nested.keys().collect::<Vec<_>>(), vec!["z", "a"]);
}

#[test]
fn model_keeps_unknown_fields_on_variant_and_nested_objects() {
    let input = SMALL
        .replacen(
            "\"shot\": {\n              \"shot\": \"shot-1\"\n            }",
            "\"shot\": {\n              \"shot\": \"shot-1\",\n              \"source_future\": 1\n            },\n            \"source_future\": 2",
            1,
        )
        .replacen(
            "\"author\": \"human\"",
            "\"author\": {\"agent\": {\"name\": \"x\", \"future\": 3}, \"author_future\": 4}",
            1,
        )
        .replacen(
            "\"fill\": \"#222222\"",
            "\"fill\": \"#222222\",\n                      \"crop\": {\"x\": 0.0, \"y\": 0.0, \"width\": 1.0, \"height\": 1.0, \"crop_future\": 6},\n                      \"layer_future\": 5",
            1,
        )
        .replacen(
            "\"cut\": {\n              \"cut\": \"cut-alt\"\n            }",
            "\"cut\": {\n              \"cut\": \"cut-alt\",\n              \"cut_source_future\": 7\n            }",
            1,
        );
    let project: Project = serde_json::from_str(&input).unwrap();
    let written = project.to_pretty_json().unwrap();
    for field in [
        "source_future",
        "author_future",
        "\"future\": 3",
        "layer_future",
        "crop_future",
        "cut_source_future",
    ] {
        assert!(written.contains(field), "missing {field}: {written}");
    }
}

#[test]
fn validate_reports_all_issues_at_once() {
    let mut project = fixture();
    project.fps = 26;
    project.format.width = 0;
    project.scenes[0].id = "bad id".into();
    let issues = validate(&project);
    assert!(issues.len() >= 3, "issues: {issues:?}");
}

#[test]
fn validate_names_valid_options() {
    let mut project = fixture();
    project.fps = 26;
    let issue = validate(&project)
        .into_iter()
        .find(|issue| issue.field == "fps")
        .expect("fps issue");
    assert_eq!(issue.valid, vec!["24", "25", "30", "60"]);
}

#[test]
fn validate_limit_scenes() {
    let mut project = fixture();
    project.scenes = std::iter::repeat_n(project.scenes[0].clone(), 501).collect();
    assert!(
        validate(&project)
            .iter()
            .any(|issue| issue.field == "scenes")
    );
}

#[test]
fn validate_limit_shots() {
    let mut project = fixture();
    let shot = project.scenes[0].shots[0].clone();
    project.scenes[0].shots = (0..1001)
        .map(|index| {
            let mut shot = shot.clone();
            shot.id = format!("first-{index}");
            shot
        })
        .collect();
    project.scenes[1].shots = (0..1000)
        .map(|index| {
            let mut shot = shot.clone();
            shot.id = format!("second-{index}");
            shot
        })
        .collect();
    assert!(
        validate(&project)
            .iter()
            .any(|issue| issue.field == "shots")
    );
}

#[test]
fn validate_limit_takes() {
    let mut project = fixture();
    project.scenes[0].shots[0].takes = (0..101)
        .map(|index| {
            let mut take = project.scenes[0].shots[0].takes[0].clone();
            take.id = format!("take-{index}");
            take
        })
        .collect();
    assert!(
        validate(&project)
            .iter()
            .any(|issue| issue.field == "scenes[0].shots[0].takes")
    );
}

#[test]
fn validate_limit_cuts() {
    let mut project = fixture();
    project.cuts = std::iter::repeat_n(project.cuts[0].clone(), 101).collect();
    assert!(validate(&project).iter().any(|issue| issue.field == "cuts"));
}

#[test]
fn validate_limit_clips() {
    let mut project = fixture();
    project.cuts[0].clips = (0..1001)
        .map(|index| {
            let mut clip = project.cuts[0].clips[0].clone();
            clip.id = format!("clip-{index}");
            clip
        })
        .collect();
    assert!(
        validate(&project)
            .iter()
            .any(|issue| issue.field == "cuts[0].clips")
    );
}

#[test]
fn validate_limit_shot_duration() {
    let mut project = fixture();
    project.scenes[0].shots[0].duration = 3601.0;
    assert!(
        validate(&project)
            .iter()
            .any(|issue| issue.field.ends_with("duration"))
    );
}

#[test]
fn validate_limit_format() {
    let mut project = fixture();
    project.format.width = 8193;
    assert!(
        validate(&project)
            .iter()
            .any(|issue| issue.field == "format.width")
    );
}

#[test]
fn validate_format_minimum_and_positions_limit() {
    let mut project = fixture();
    project.format.width = 0;
    project.layout.positions.get_mut("shot:shot-1").unwrap().x = 100_001.0;
    let issues = validate(&project);
    assert!(issues.iter().any(|issue| issue.field == "format.width"));
    assert!(
        issues
            .iter()
            .any(|issue| issue.field == "layout.positions.shot:shot-1.x")
    );
}

#[test]
fn validate_allows_cross_kind_ids_with_typed_layout_keys() {
    let mut project = fixture();
    project.stickies[0].id = "shot-1".into();
    project.layout.positions.insert(
        "sticky:shot-1".into(),
        storyboard_core::Position {
            x: 11.0,
            y: 12.0,
            extra: serde_json::Map::new(),
        },
    );

    assert!(validate(&project).is_empty());
    assert!(project.layout.positions.contains_key("shot:shot-1"));
    assert!(project.layout.positions.contains_key("sticky:shot-1"));
}

#[test]
fn validate_requires_typed_layout_keys() {
    let mut project = fixture();
    let position = project.layout.positions.remove("shot:shot-1").unwrap();
    project.layout.positions.insert("shot-1".into(), position);

    let issues = validate(&project);
    assert!(
        issues
            .iter()
            .any(|issue| issue.field == "layout.positions.shot-1")
    );
}

#[test]
fn validate_limit_geometry() {
    let mut project = fixture();
    project.scenes[0].shots[0].takes[0].panels[0].layers[0].x = 1_000_001.0;
    project.scenes[0].shots[0].takes[0].panels[0].layers[0].width = 1_000_001.0;
    project.scenes[0].shots[0].takes[0].panels[0].layers[0].height = 1_000_001.0;
    let issues = validate(&project);
    for field in [
        "scenes[0].shots[0].takes[0].panels[0].layers[0].x",
        "scenes[0].shots[0].takes[0].panels[0].layers[0].width",
        "scenes[0].shots[0].takes[0].panels[0].layers[0].height",
    ] {
        assert!(issues.iter().any(|issue| issue.field == field), "{field}");
    }
}

#[test]
fn validate_limit_text() {
    let mut project = fixture();
    project.scenes[0].shots[0].action = "x".repeat(20_001);
    assert!(
        validate(&project)
            .iter()
            .any(|issue| issue.field.ends_with("action"))
    );
}

#[test]
fn validate_text_and_paths_count_characters_not_bytes() {
    let mut project = fixture();
    project.title = "é".repeat(20_000);
    project.scenes[0].title = "é".repeat(20_000);
    project.scenes[0].shots[0].refs = vec!["é".repeat(1_000)];
    let issues = validate(&project);
    assert!(!issues.iter().any(|issue| issue.field == "title"));
    assert!(!issues.iter().any(|issue| issue.field == "scenes[0].title"));
    assert!(
        !issues
            .iter()
            .any(|issue| issue.field == "scenes[0].shots[0].refs[0]")
    );
}

#[test]
fn validate_every_text_field_and_crop_geometry() {
    let mut project = fixture();
    project.title = "x".repeat(20_001);
    project.scenes[0].title = "x".repeat(20_001);
    let layer = &mut project.scenes[0].shots[0].takes[0].panels[0].layers[0];
    layer.crop = Some(storyboard_core::Crop {
        x: 0.0,
        y: 0.0,
        width: 1_000_001.0,
        height: 1.0,
        extra: Default::default(),
    });
    let issues = validate(&project);
    assert!(issues.iter().any(|issue| issue.field == "title"));
    assert!(issues.iter().any(|issue| issue.field == "scenes[0].title"));
    assert!(
        issues
            .iter()
            .any(|issue| issue.field.ends_with("crop.width"))
    );
}

#[test]
fn validate_ids_and_paths_have_hard_limits() {
    let mut project = fixture();
    project.scenes[0].id = "x".repeat(161);
    project.scenes[0].shots[0].refs = vec!["a".repeat(1001)];
    let issues = validate(&project);
    assert!(issues.iter().any(|issue| issue.field == "scenes[0].id"));
    assert!(
        issues
            .iter()
            .any(|issue| issue.field == "scenes[0].shots[0].refs[0]")
    );
}

#[test]
fn validate_limit_font_size() {
    let mut project = fixture();
    project.scenes[0].shots[0].takes[0].panels[0].layers[0].font_size = Some(4097.0);
    assert!(
        validate(&project)
            .iter()
            .any(|issue| issue.field.ends_with("font_size"))
    );
}

#[test]
fn validate_limit_author_name() {
    let mut project = fixture();
    project.stickies[0].author = storyboard_core::Author::Agent {
        agent: storyboard_core::AgentAuthor {
            name: "x".repeat(201),
            extra: Default::default(),
        },
        extra: Default::default(),
    };
    assert!(
        validate(&project)
            .iter()
            .any(|issue| issue.field.ends_with("author.agent.name"))
    );
}

#[test]
fn stroke_points_stay_on_one_line_at_any_length() {
    let mut project = fixture();
    project.scenes[0].shots[0].takes[0].panels[0].strokes[0].points =
        (0..100).map(|value| [value as f64, value as f64]).collect();
    let written = project.to_pretty_json().unwrap();
    let points = written
        .lines()
        .find(|line| line.contains("\"points\""))
        .expect("points line");
    assert!(points.contains("[99.0,99.0]"));
}

#[test]
fn only_stroke_points_are_compacted() {
    let mut project = fixture();
    project.scenes[0].shots[0].takes[0].panels[0]
        .extra
        .insert("points_hint".into(), serde_json::json!([1, 2]));
    let written = project.to_pretty_json().unwrap();
    assert!(written.contains("\"points\": [[1.0,2.0],[3.0,4.0]]"));
    assert!(
        written.contains("\"points_hint\": [\n"),
        "only a stroke's points field may be compact: {written}"
    );
}

#[test]
fn point_compaction_applies_only_to_a_real_strokes_points() {
    let mut project = fixture();
    project.scenes[0].shots[0]
        .extra
        .insert("points".into(), serde_json::json!([1, 2]));
    project.scenes[0].shots[0].takes[0].panels[0].strokes[0]
        .extra
        .insert("future".into(), serde_json::json!({"points": [3, 4]}));

    let written = project.to_pretty_json().unwrap();
    assert!(written.contains("\"points\": [[1.0,2.0],[3.0,4.0]]"));
    assert!(
        written.contains("\"points\": [\n            1,"),
        "{written}"
    );
    assert!(
        written.contains("\"future\": {\n                        \"points\": [\n"),
        "{written}"
    );
}

#[test]
fn validate_shot_limit_counts_duplicate_ids() {
    let mut project = fixture();
    let duplicate = project.scenes[0].shots[0].clone();
    project.scenes[0].shots = std::iter::repeat_n(duplicate, 2001).collect();
    assert!(
        validate(&project)
            .iter()
            .any(|issue| issue.field == "shots")
    );
}

#[test]
fn validate_later_sticky_reply_is_valid() {
    let mut project = fixture();
    project.stickies[0].reply_to = Some("sticky-2".into());
    let mut later = project.stickies[0].clone();
    later.id = "sticky-2".into();
    later.reply_to = None;
    project.stickies.push(later);
    assert!(
        !validate(&project)
            .iter()
            .any(|issue| issue.field == "stickies[0].reply_to")
    );
}

#[test]
fn validate_reference_options_are_complete_and_scoped() {
    let mut project = fixture();
    project.cuts[0].clips[0].source =
        storyboard_core::ClipSource::Cut(storyboard_core::CutSource {
            cut: "missing-cut".into(),
            extra: Default::default(),
            source_extra: Default::default(),
        });
    if let storyboard_core::ClipSource::Shot(source) = &mut project.cuts[1].clips[0].source {
        source.take = Some("take-3".into());
    }
    let issues = validate(&project);
    let unknown_cut = issues
        .iter()
        .find(|issue| issue.field == "cuts[0].clips[0].source.cut")
        .expect("unknown cut issue");
    assert_eq!(unknown_cut.valid, ["cut-alt", "cut-main"]);
    let bad_take = issues
        .iter()
        .find(|issue| issue.field == "cuts[1].clips[0].source.shot.take")
        .expect("bad take issue");
    assert_eq!(bad_take.valid, ["take-4"]);
    assert!(!bad_take.valid.contains(&"take-3".to_owned()));
}

#[test]
fn validate_pin_reference_options_are_scoped() {
    let cases = [
        ("take:shot-1/missing", ["take-1", "take-2"].as_slice()),
        ("panel:shot-1/take-1/missing", ["panel-1"].as_slice()),
        (
            "layer:shot-1/take-1/panel-1/missing",
            ["layer-1"].as_slice(),
        ),
        ("clip:cut-main/missing", ["clip-1", "clip-2"].as_slice()),
        (
            "clip:missing-cut/missing",
            ["cut-alt", "cut-main"].as_slice(),
        ),
    ];
    for (pin, expected) in cases {
        let mut project = fixture();
        project.stickies[0].pin = storyboard_core::Pin::parse(pin).unwrap();
        let issue = validate(&project)
            .into_iter()
            .find(|issue| issue.field == "stickies[0].pin")
            .expect(pin);
        assert_eq!(issue.valid, expected, "{pin}");
    }
}

#[test]
fn validate_and_render_color_syntax_agree() {
    for value in ["ff0000", "abc", "hsv(0 100% 100%)", "oklch(0.6 0.2 30)"] {
        let mut project = fixture();
        project.scenes[0].shots[0].takes[0].panels[0].background = value.into();
        assert!(
            validate(&project)
                .iter()
                .any(|issue| issue.field.ends_with("background")),
            "{value} must be rejected by the shared renderer-compatible parser"
        );
    }
}

#[test]
fn tagged_wire_refuses_ambiguous_source_and_accepts_null_take() {
    let both = SMALL.replacen(
        "\"shot\": {\n              \"shot\": \"shot-1\"\n            }",
        "\"shot\": {\"shot\": \"shot-1\"}, \"cut\": {\"cut\": \"cut-alt\"}",
        1,
    );
    assert!(serde_json::from_str::<Project>(&both).is_err());
    let null_take = SMALL.replacen(
        "\"shot\": {\n              \"shot\": \"shot-1\"\n            }",
        "\"shot\": {\"shot\": \"shot-1\", \"take\": null}",
        1,
    );
    assert!(serde_json::from_str::<Project>(&null_take).is_ok());
}

#[cfg(feature = "schema")]
#[test]
fn tagged_wire_schema_matches_reader() {
    let schema = serde_json::to_value(schemars::schema_for!(storyboard_core::ClipSource)).unwrap();
    let branches = schema["oneOf"].as_array().expect("tagged union branches");
    assert_eq!(branches.len(), 2);
    assert!(branches.iter().all(|branch| branch.get("not").is_some()));
    let shot = &branches[0]["properties"]["shot"]["properties"]["take"];
    assert!(
        shot.to_string().contains("null"),
        "null take must be in schema: {shot}"
    );
}

#[test]
fn validate_references_and_reference_options() {
    let mut project: Project = serde_json::from_str(&SMALL.replace(
        "\"pin\": \"panel:shot-1/take-1/panel-1\"",
        "\"pin\": \"panel:shot-1/take-1/missing\"",
    ))
    .unwrap();
    project.scenes[0].shots[0].selected_take.clear();
    if let storyboard_core::ClipSource::Shot(source) = &mut project.cuts[0].clips[0].source {
        source.take = Some("take-3".into());
    }
    project.stickies[0].reply_to = Some("missing-sticky".into());
    project.cuts[0].clips[1].source =
        storyboard_core::ClipSource::Cut(storyboard_core::CutSource {
            cut: "missing-cut".into(),
            extra: Default::default(),
            source_extra: Default::default(),
        });
    project.active_cut = Some("missing-active".into());
    let issues = validate(&project);
    for field in [
        "scenes[0].shots[0].selected_take",
        "cuts[0].clips[0].source.shot.take",
        "stickies[0].pin",
        "stickies[0].reply_to",
        "cuts[0].clips[1].source.cut",
        "active_cut",
    ] {
        let issue = issues
            .iter()
            .find(|issue| issue.field == field)
            .expect(field);
        assert!(!issue.valid.is_empty(), "{field} must name valid options");
    }
}

#[test]
fn validate_utc_colors_and_required_layer_values() {
    let mut project = fixture();
    project.stickies[0].created = "2026-11-07T00:00:00-08:00".into();
    let panel = &mut project.scenes[0].shots[0].takes[0].panels[0];
    panel.strokes[0].color.clear();
    panel.layers[0].fill = Some(String::new());
    let mut image = panel.layers[0].clone();
    image.id = "image-layer".into();
    image.layer_type = "image".into();
    image.src = None;
    panel.layers.push(image);
    let issues = validate(&project);
    for field in [
        "stickies[0].created",
        "scenes[0].shots[0].takes[0].panels[0].strokes[0].color",
        "scenes[0].shots[0].takes[0].panels[0].layers[0].fill",
        "scenes[0].shots[0].takes[0].panels[0].layers[1].src",
    ] {
        assert!(
            issues.iter().any(|issue| issue.field == field),
            "missing {field}"
        );
    }
}

#[test]
fn validate_clip_bounds_slot_opacity_and_size() {
    let mut project = fixture();
    let clip = &mut project.cuts[0].clips[0];
    clip.in_frame = 10;
    clip.out_frame = Some(9);
    clip.slot = Some(0);
    let layer = &mut project.scenes[0].shots[0].takes[0].panels[0].layers[0];
    layer.opacity = 1.1;
    layer.width = -1.0;
    layer.height = -1.0;
    let issues = validate(&project);
    for suffix in [".in", ".slot", ".opacity", ".width", ".height"] {
        assert!(
            issues.iter().any(|issue| issue.field.ends_with(suffix)),
            "missing {suffix}"
        );
    }
}
