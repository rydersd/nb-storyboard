use storyboard_core::{
    Clip, ClipSource, CompileError, Cut, CutSource, ExtraFields, Panel, Project, ShotSource,
    compile_active, compile_cut, compile_story, sample, validate,
};

const SMALL: &str = include_str!("fixtures/small.json");

fn fixture() -> Project {
    serde_json::from_str(SMALL).expect("the small fixture is a project")
}

fn shot_clip(id: impl Into<String>, shot: &str) -> Clip {
    Clip {
        id: id.into(),
        source: ClipSource::Shot(ShotSource {
            shot: shot.into(),
            take: None,
            extra: ExtraFields::default(),
            source_extra: ExtraFields::default(),
        }),
        in_frame: 0,
        out_frame: None,
        slot: None,
        extra: ExtraFields::default(),
    }
}

fn cut_clip(id: impl Into<String>, cut: &str) -> Clip {
    Clip {
        id: id.into(),
        source: ClipSource::Cut(CutSource {
            cut: cut.into(),
            extra: ExtraFields::default(),
            source_extra: ExtraFields::default(),
        }),
        in_frame: 0,
        out_frame: None,
        slot: None,
        extra: ExtraFields::default(),
    }
}

fn cut(id: impl Into<String>, clips: Vec<Clip>) -> Cut {
    Cut {
        id: id.into(),
        name: String::new(),
        clips,
        extra: ExtraFields::default(),
    }
}

#[test]
fn compile_story_order_uses_selected_takes() {
    let mut project = fixture();
    project.scenes[0].shots[0].selected_take = "take-2".into();
    let compiled = compile_story(&project).unwrap();
    let choices: Vec<_> = compiled
        .items
        .iter()
        .map(|item| (item.shot.as_str(), item.take.as_str()))
        .collect();
    assert_eq!(
        choices,
        [
            ("shot-1", "take-2"),
            ("shot-2", "take-3"),
            ("shot-3", "take-4"),
            ("shot-4", "take-5"),
        ]
    );
    assert_eq!(compiled.frames, 120);
}

#[test]
fn compile_cut_trims() {
    let mut project = fixture();
    let clip = &mut project.cuts[1].clips[0];
    clip.in_frame = 5;
    clip.out_frame = Some(10);
    let compiled = compile_cut(&project, "cut-alt").unwrap();
    assert_eq!(compiled.frames, 5);
    assert_eq!(compiled.items.len(), 1);
    assert_eq!(compiled.items[0].start, 0);
    assert_eq!(compiled.items[0].end, 5);
    assert_eq!(compiled.items[0].offset, 5);
}

#[test]
fn compile_slot_pads_and_cuts() {
    let mut project = fixture();
    project.cuts[1].clips[0].slot = Some(30);
    let padded = compile_cut(&project, "cut-alt").unwrap();
    assert_eq!(padded.frames, 30);
    assert!(sample(&padded, 23).is_some());
    assert!(
        sample(&padded, 24).is_none(),
        "the remainder is black padding"
    );

    project.cuts[1].clips[0].slot = Some(10);
    let cut = compile_cut(&project, "cut-alt").unwrap();
    assert_eq!(cut.frames, 10);
    assert_eq!(cut.items[0].end, 10);
    assert!(sample(&cut, 9).is_some());
    assert!(sample(&cut, 10).is_none());
}

#[test]
fn compile_nested_expands_in_place() {
    let project = fixture();
    let compiled = compile_cut(&project, "cut-main").unwrap();
    assert_eq!(compiled.frames, 72);
    assert_eq!(compiled.items.len(), 2);
    assert_eq!(compiled.items[0].shot, "shot-1");
    assert_eq!((compiled.items[0].start, compiled.items[0].end), (0, 48));
    assert_eq!(compiled.items[1].shot, "shot-3");
    assert_eq!((compiled.items[1].start, compiled.items[1].end), (48, 72));
}

#[test]
fn compile_cycle_is_error() {
    let mut project = fixture();
    project.cuts[1].clips[0].source = ClipSource::Cut(CutSource {
        cut: "cut-main".into(),
        extra: ExtraFields::default(),
        source_extra: ExtraFields::default(),
    });
    assert_eq!(
        compile_active(&project),
        Err(CompileError::Cycle {
            cuts: vec!["cut-main".into(), "cut-alt".into(), "cut-main".into()],
        })
    );
}

#[test]
fn compile_sample_each_frame() {
    let project = fixture();
    let compiled = compile_cut(&project, "cut-alt").unwrap();
    for frame in 0..24 {
        let item = sample(&compiled, frame).expect("the shot occupies every source frame");
        assert_eq!(item.shot, "shot-3");
        assert_eq!(item.offset, frame);
    }
    assert!(sample(&compiled, 24).is_none());
}

fn nested_cuts(depth: usize) -> Project {
    let mut project = fixture();
    project.cuts.clear();
    for index in (0..depth).rev() {
        let id = format!("cut-{index}");
        let clip = if index + 1 == depth {
            shot_clip(format!("clip-{index}"), "shot-1")
        } else {
            cut_clip(format!("clip-{index}"), &format!("cut-{}", index + 1))
        };
        project.cuts.insert(0, cut(id, vec![clip]));
    }
    project.active_cut = Some("cut-0".into());
    project
}

fn wide_cut(repetitions: usize) -> Project {
    let mut project = fixture();
    let child = cut(
        "cut-child",
        (0..1000)
            .map(|index| shot_clip(format!("child-{index}"), "shot-1"))
            .collect(),
    );
    let root = cut(
        "cut-root",
        (0..repetitions)
            .map(|index| cut_clip(format!("root-{index}"), "cut-child"))
            .collect(),
    );
    project.cuts = vec![root, child];
    project.active_cut = Some("cut-root".into());
    project
}

#[test]
fn compile_enforces_depth_item_and_duration_limits() {
    assert_eq!(compile_active(&nested_cuts(20)).unwrap().items.len(), 1);
    assert!(compile_active(&nested_cuts(21)).is_err());
    assert!(
        validate(&nested_cuts(21))
            .iter()
            .any(|issue| issue.message.contains("nesting depth"))
    );

    let wide = wide_cut(10);
    let first = compile_active(&wide).unwrap();
    let second = compile_active(&wide).unwrap();
    assert_eq!(first, second, "wide compilation must be deterministic");
    assert_eq!(first.items.len(), 10_000);
    assert!(
        first.items.capacity() <= 10_000,
        "compiled output is bounded"
    );
    assert!(compile_active(&wide_cut(11)).is_err());
    assert!(
        validate(&wide_cut(11))
            .iter()
            .any(|issue| issue.message.contains("10000 compiled items"))
    );

    let mut long = nested_cuts(2);
    long.scenes[0].shots[0].duration = 3600.0;
    long.cuts[0].clips = (0..10)
        .map(|index| cut_clip(format!("long-{index}"), "cut-1"))
        .collect();
    assert_eq!(compile_active(&long).unwrap().frames, 864_000);
    long.cuts[0].clips.push(cut_clip("long-10", "cut-1"));
    assert!(compile_active(&long).is_err());
    assert!(
        validate(&long)
            .iter()
            .any(|issue| issue.message.contains("10 hours"))
    );
}

#[test]
fn compile_story_refuses_more_than_ten_thousand_items() {
    let mut project = fixture();
    let shot = project.scenes[0].shots[0].clone();
    project.scenes.truncate(1);
    project.scenes[0].shots = vec![shot; 10_001];

    assert!(matches!(
        compile_story(&project),
        Err(CompileError::Limit { ref field, .. }) if field == "shots"
    ));
    assert!(
        validate(&project)
            .iter()
            .any(|issue| issue.field == "shots")
    );
}

#[test]
fn compile_extreme_timing_values_are_safe() {
    let mut project = fixture();
    let clip = &mut project.cuts[1].clips[0];
    clip.in_frame = u64::MAX;
    clip.out_frame = Some(u64::MAX);
    let compiled = compile_cut(&project, "cut-alt").unwrap();
    assert_eq!(compiled.frames, 0);
    assert!(!compiled.warnings.is_empty());

    project.cuts[1].clips[0].slot = Some(u64::MAX);
    assert!(compile_cut(&project, "cut-alt").is_err());

    let mut huge_duration = fixture();
    huge_duration.scenes[0].shots[0].duration = f64::MAX;
    assert!(compile_story(&huge_duration).is_err());
}

#[test]
fn compile_empty_story_and_cut_are_zero_frames() {
    let mut project = fixture();
    project.scenes.clear();
    project.cuts = vec![cut("empty", Vec::new())];
    assert_eq!(compile_story(&project).unwrap().frames, 0);
    assert_eq!(compile_cut(&project, "empty").unwrap().frames, 0);
}

fn trimmed_nested_cuts(depth: usize, width: usize) -> Project {
    let mut project = fixture();
    project.cuts.clear();
    for level in (0..depth).rev() {
        let clips = (0..width)
            .map(|index| {
                let mut clip = if level + 1 == depth {
                    shot_clip(format!("clip-{level}-{index}"), "shot-1")
                } else {
                    cut_clip(
                        format!("clip-{level}-{index}"),
                        &format!("cut-{}", level + 1),
                    )
                };
                clip.out_frame = Some(1);
                clip
            })
            .collect();
        project.cuts.insert(0, cut(format!("cut-{level}"), clips));
    }
    project.active_cut = Some("cut-0".into());
    project
}

#[test]
fn compile_refuses_twenty_levels_of_one_thousand_trimmed_clips() {
    assert!(matches!(
        compile_active(&trimmed_nested_cuts(20, 1000)),
        Err(CompileError::Limit { ref field, .. }) if field == "compiled items"
    ));
}

#[test]
fn compile_refuses_all_black_padding_tree_by_expanded_clip_count() {
    let mut project = fixture();
    project.cuts = (0..8)
        .map(|level| {
            let clips = (0..10)
                .map(|index| {
                    let mut clip = if level == 7 {
                        shot_clip(format!("leaf-{level}-{index}"), "shot-1")
                    } else {
                        cut_clip(
                            format!("branch-{level}-{index}"),
                            &format!("padding-{}", level + 1),
                        )
                    };
                    clip.out_frame = Some(0);
                    clip.slot = Some(1);
                    clip
                })
                .collect();
            cut(format!("padding-{level}"), clips)
        })
        .collect();
    project.active_cut = Some("padding-0".into());

    assert!(matches!(
        compile_active(&project),
        Err(CompileError::Limit { ref field, .. }) if field == "compiled items"
    ));
}

fn distinctly_trimmed_nested_cuts(depth: usize, width: usize) -> Project {
    let mut project = fixture();
    project.scenes[0].shots[0].duration = 3600.0;
    project.cuts.clear();
    for level in (0..depth).rev() {
        let clips = (0..width)
            .map(|index| {
                let mut clip = if level + 1 == depth {
                    shot_clip(format!("distinct-{level}-{index}"), "shot-1")
                } else {
                    cut_clip(
                        format!("distinct-{level}-{index}"),
                        &format!("distinct-cut-{}", level + 1),
                    )
                };
                clip.in_frame = index as u64;
                clip.out_frame = Some(index as u64 + 1);
                clip
            })
            .collect();
        project
            .cuts
            .insert(0, cut(format!("distinct-cut-{level}"), clips));
    }
    project.active_cut = Some("distinct-cut-0".into());
    project
}

#[test]
fn compile_refuses_twenty_levels_of_one_thousand_distinct_trims() {
    assert!(matches!(
        compile_active(&distinctly_trimmed_nested_cuts(20, 1000)),
        Err(CompileError::Limit { ref field, .. }) if field == "compiled items"
    ));
}

#[test]
fn compile_preflight_cycle_check_rejects_cycles() {
    let mut project = fixture();
    project.cuts = vec![cut("cycle", vec![cut_clip("cycle-clip", "cycle")])];
    assert!(matches!(
        compile_cut(&project, "cycle"),
        Err(CompileError::Cycle { .. })
    ));
}

#[test]
fn compile_preflight_depth_check_uses_true_height_in_both_clip_orders() {
    for direct_shot_first in [true, false] {
        let mut project = nested_cuts(24);
        let root = &mut project.cuts[0];
        let direct = shot_clip("root-shot", "shot-1");
        if direct_shot_first {
            root.clips.insert(0, direct);
        } else {
            root.clips.push(direct);
        }
        assert!(matches!(
            compile_active(&project),
            Err(CompileError::Limit { ref field, .. }) if field == "nesting depth"
        ));
    }
}

#[test]
fn compile_preflight_item_check_rejects_expansion() {
    assert!(matches!(
        compile_active(&wide_cut(11)),
        Err(CompileError::Limit { ref field, .. }) if field == "compiled items"
    ));
}

fn legal_ten_thousand_items_at_depth_seven() -> Project {
    let mut project = fixture();
    project.cuts.clear();
    project.cuts.push(cut(
        "legal-0",
        (0..10)
            .map(|index| cut_clip(format!("root-{index}"), "legal-1"))
            .collect(),
    ));
    for level in 1..6 {
        project.cuts.push(cut(
            format!("legal-{level}"),
            vec![cut_clip(
                format!("link-{level}"),
                &format!("legal-{}", level + 1),
            )],
        ));
    }
    project.cuts.push(cut(
        "legal-6",
        (0..1000)
            .map(|index| shot_clip(format!("leaf-{index}"), "shot-1"))
            .collect(),
    ));
    project.active_cut = Some("legal-0".into());
    project
}

#[test]
fn compile_legal_ten_thousand_items_at_nesting_seven() {
    let project = legal_ten_thousand_items_at_depth_seven();
    assert!(validate(&project).is_empty());
    assert_eq!(compile_active(&project).unwrap().items.len(), 10_000);
}

#[test]
fn compile_thirty_thousand_cut_chain_returns_a_limit_error_without_stack_overflow() {
    let mut project = fixture();
    project.cuts = (0..30_000)
        .map(|index| {
            let clip = if index == 29_999 {
                shot_clip(format!("chain-clip-{index}"), "shot-1")
            } else {
                cut_clip(
                    format!("chain-clip-{index}"),
                    &format!("chain-{}", index + 1),
                )
            };
            cut(format!("chain-{index}"), vec![clip])
        })
        .collect();
    project.active_cut = Some("chain-0".into());
    assert!(matches!(
        compile_active(&project),
        Err(CompileError::Limit { ref field, .. }) if field == "cuts"
    ));
}

#[test]
fn validate_analyzes_a_thirty_thousand_cut_chain_once() {
    let mut project = fixture();
    project.cuts = (0..30_000)
        .map(|index| {
            let clip = if index == 29_999 {
                shot_clip(format!("validate-chain-clip-{index}"), "shot-1")
            } else {
                cut_clip(
                    format!("validate-chain-clip-{index}"),
                    &format!("validate-chain-{}", index + 1),
                )
            };
            cut(format!("validate-chain-{index}"), vec![clip])
        })
        .collect();

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| issue.field == "cuts"));
    assert!(
        !issues.iter().any(|issue| issue.field.ends_with(".compile")),
        "count breaches must skip compile analysis: {issues:?}"
    );
}

#[test]
fn compile_cycle_precedes_nesting_limit() {
    let mut project = nested_cuts(25);
    project.cuts.last_mut().unwrap().clips[0] = cut_clip("cycle-back", "cut-0");
    let error = compile_active(&project).unwrap_err();
    assert!(
        matches!(error, CompileError::Cycle { .. }),
        "cycle must win over depth: {error:?}"
    );
}

#[test]
fn validate_reports_cut_cycles() {
    let mut project = fixture();
    project.cuts = vec![cut("cycle", vec![cut_clip("cycle-clip", "cycle")])];
    let issues = validate(&project);
    assert!(
        issues
            .iter()
            .any(|issue| issue.field == "cuts[0].compile" && issue.message.contains("cycle")),
        "issues: {issues:?}"
    );
}

#[test]
fn validate_collects_duration_limit_after_unknown_reference() {
    let mut project = fixture();
    project.scenes[0].shots[0].duration = 3600.0;
    project.cuts = vec![cut(
        "problem",
        std::iter::once(shot_clip("missing", "no-such-shot"))
            .chain((0..11).map(|index| shot_clip(format!("long-{index}"), "shot-1")))
            .collect(),
    )];
    let issues = validate(&project);
    assert!(
        issues
            .iter()
            .any(|issue| issue.field == "cuts[0].clips[0].source.shot"),
        "issues: {issues:?}"
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.field == "cuts[0].compile" && issue.message.contains("10 hours")),
        "issues: {issues:?}"
    );
}

#[test]
fn compile_warnings_are_deduped_and_bounded() {
    let mut project = wide_cut(10);
    for cut in &mut project.cuts {
        for clip in &mut cut.clips {
            clip.out_frame = None;
            clip.slot = Some(1);
        }
    }
    let compiled = compile_active(&project).unwrap();
    assert!(
        compiled.warnings.len() <= 1_001,
        "warning output must be capped: {} warnings",
        compiled.warnings.len()
    );
    assert_eq!(
        compiled
            .warnings
            .iter()
            .filter(|warning| warning.field == "compile.warnings")
            .count(),
        1,
        "suppressed warnings need one summary: {:?}",
        compiled.warnings
    );
}

#[test]
fn compile_clip_take_overrides_the_selected_take() {
    let mut project = fixture();
    project.scenes[0].shots[0].selected_take = "take-2".into();
    project.cuts = vec![cut(
        "explicit-take",
        vec![Clip {
            source: ClipSource::Shot(ShotSource {
                shot: "shot-1".into(),
                take: Some("take-1".into()),
                extra: ExtraFields::default(),
                source_extra: ExtraFields::default(),
            }),
            ..shot_clip("explicit", "shot-1")
        }],
    )];
    assert_eq!(
        compile_cut(&project, "explicit-take").unwrap().items[0].take,
        "take-1"
    );
}

#[test]
fn compile_sample_reports_non_zero_offset_panel_and_clip() {
    let mut project = fixture();
    project.scenes[1].shots[0].takes[0].panels.push(Panel {
        id: "panel-later".into(),
        time: 0.5,
        title: String::new(),
        direction: String::new(),
        background: String::new(),
        layers: Vec::new(),
        strokes: Vec::new(),
        extra: ExtraFields::default(),
    });
    project.cuts[1].clips[0].in_frame = 5;
    let compiled = compile_cut(&project, "cut-alt").unwrap();
    assert_eq!(compiled.items[0].clip.as_deref(), Some("clip-3"));
    let sampled = sample(&compiled, 7).unwrap();
    assert_eq!(sampled.offset, 12);
    assert_eq!(sampled.panel, Some("panel-later"));
}

#[test]
fn compile_duration_limit_accepts_ten_hours_and_rejects_ten_hours_plus_one_second() {
    let mut project = fixture();
    project.scenes[0].shots[0].duration = 3600.0;
    project.cuts = vec![cut(
        "duration",
        (0..10)
            .map(|index| shot_clip(format!("ten-hours-{index}"), "shot-1"))
            .collect(),
    )];
    assert_eq!(compile_cut(&project, "duration").unwrap().frames, 864_000);
    project.cuts[0]
        .clips
        .push(shot_clip("one-second-more", "shot-2"));
    assert!(matches!(
        compile_cut(&project, "duration"),
        Err(CompileError::Limit { ref field, .. }) if field == "sequence duration"
    ));
}

#[test]
fn validate_reports_story_duration_limit() {
    let mut project = fixture();
    let mut long_shots = Vec::new();
    for index in 0..11 {
        let mut shot = project.scenes[0].shots[0].clone();
        shot.id = format!("story-long-{index}");
        shot.duration = 3600.0;
        shot.selected_take = format!("story-take-{index}");
        shot.takes[0].id = shot.selected_take.clone();
        long_shots.push(shot);
    }
    project.scenes = vec![project.scenes[0].clone()];
    project.scenes[0].shots = long_shots;
    project.cuts.clear();
    let issues = validate(&project);
    assert!(
        issues
            .iter()
            .any(|issue| issue.field == "story.compile" && issue.message.contains("10 hours")),
        "issues: {issues:?}"
    );
}

fn project_with_unknown_selected_take_and_duration_breach() -> Project {
    let mut project = fixture();
    let mut invalid = project.scenes[0].shots[0].clone();
    invalid.id = "invalid-selected-take".into();
    invalid.duration = 3600.0;
    invalid.selected_take = "missing-take".into();

    let mut shots = vec![invalid];
    for index in 0..11 {
        let mut shot = project.scenes[0].shots[0].clone();
        shot.id = format!("long-shot-{index}");
        shot.duration = 3600.0;
        shot.selected_take = format!("long-take-{index}");
        shot.takes[0].id = shot.selected_take.clone();
        shots.push(shot);
    }
    project.scenes = vec![project.scenes[0].clone()];
    project.scenes[0].shots = shots;
    project.cuts = vec![cut(
        "long-cut",
        std::iter::once(shot_clip("invalid-clip", "invalid-selected-take"))
            .chain((0..11).map(|index| {
                shot_clip(format!("long-clip-{index}"), &format!("long-shot-{index}"))
            }))
            .collect(),
    )];
    project
}

#[test]
fn validate_projection_treats_unknown_selected_takes_as_zero_cost() {
    let project = project_with_unknown_selected_take_and_duration_breach();
    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.field == "scenes[0].shots[0].selected_take" && issue.message.contains("selected take")
    }));
    for field in ["story.compile", "cuts[0].compile"] {
        assert_eq!(
            issues
                .iter()
                .filter(|issue| issue.field == field && issue.message.contains("10 hours"))
                .count(),
            1,
            "issues: {issues:?}"
        );
    }
}

#[test]
fn validate_projected_compile_issues_are_not_duplicated() {
    let mut project = fixture();
    project.scenes[0].shots[0].duration = 3600.0;
    project.cuts = vec![cut(
        "duplicate-check",
        (0..11)
            .map(|index| shot_clip(format!("long-{index}"), "shot-1"))
            .chain(std::iter::once(shot_clip("missing", "no-such-shot")))
            .collect(),
    )];
    let issues = validate(&project);
    assert_eq!(
        issues
            .iter()
            .filter(|issue| {
                issue.field == "cuts[0].compile" && issue.message.contains("10 hours")
            })
            .count(),
        1,
        "issues: {issues:?}"
    );
}

#[test]
fn compile_rounds_shot_duration_to_nearest_frame() {
    let mut project = fixture();
    project.scenes.truncate(1);
    project.scenes[0].shots.truncate(1);
    project.scenes[0].shots[0].duration = 1.5 / 24.0;
    assert_eq!(compile_story(&project).unwrap().frames, 2);
}

#[test]
fn compile_preflight_refuses_too_many_takes_on_a_shot() {
    let mut project = fixture();
    let shot = &mut project.scenes[0].shots[0];
    let template = shot.takes[0].clone();
    shot.takes = (0..101)
        .map(|index| {
            let mut take = template.clone();
            take.id = format!("take-many-{index}");
            take
        })
        .collect();
    shot.selected_take = "take-many-0".into();
    assert!(matches!(
        compile_story(&project),
        Err(CompileError::Limit { ref field, .. }) if field.ends_with(".takes")
    ));
}
