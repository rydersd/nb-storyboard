//! Deterministic sequence compilation for story order and cuts.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use crate::{Clip, ClipSource, Project};

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static ANALYSIS_STEPS: Cell<usize> = const { Cell::new(0) };
    static ANALYSIS_STEP_LIMIT: Cell<Option<usize>> = const { Cell::new(None) };
}

#[cfg(test)]
fn analysis_step() {
    ANALYSIS_STEPS.with(|steps| {
        let next = steps.get().saturating_add(1);
        steps.set(next);
        ANALYSIS_STEP_LIMIT.with(|limit| {
            assert!(
                limit.get().is_none_or(|maximum| next <= maximum),
                "cut analysis exceeded the test step limit of {:?}",
                limit.get()
            );
        });
    });
}

#[cfg(not(test))]
fn analysis_step() {}

#[cfg(test)]
fn reset_analysis_steps() {
    ANALYSIS_STEPS.with(|steps| steps.set(0));
}

#[cfg(test)]
fn analysis_steps() -> usize {
    ANALYSIS_STEPS.with(Cell::get)
}

#[cfg(test)]
struct AnalysisStepGuard;

#[cfg(test)]
impl Drop for AnalysisStepGuard {
    fn drop(&mut self) {
        ANALYSIS_STEP_LIMIT.with(|limit| limit.set(None));
    }
}

#[cfg(test)]
fn limit_analysis_steps(maximum: usize) -> AnalysisStepGuard {
    ANALYSIS_STEP_LIMIT.with(|limit| limit.set(Some(maximum)));
    AnalysisStepGuard
}

pub const MAX_CUT_NESTING: usize = 20;
pub const MAX_COMPILED_ITEMS: usize = 10_000;
pub const MAX_COMPILE_WARNINGS: usize = 256;
pub(crate) const MAX_SCENES: usize = 500;
pub(crate) const MAX_SHOTS: usize = 2_000;
pub(crate) const MAX_CUTS: usize = 100;
pub(crate) const MAX_CLIPS_PER_CUT: usize = 1_000;
const MAX_SEQUENCE_SECONDS: u64 = 10 * 60 * 60;

/// One contiguous interval of a shot in a compiled sequence. `end` is exclusive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledItem {
    pub start: u64,
    pub end: u64,
    pub shot: String,
    pub take: String,
    pub offset: u64,
    /// The clip that placed the item, or `None` for story order.
    pub clip: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileWarning {
    pub field: String,
    pub message: String,
}

/// A compiled sequence. Frames not covered by an item are black padding.
#[derive(Clone, Debug, PartialEq)]
pub struct Compiled {
    pub fps: u32,
    pub frames: u64,
    pub items: Vec<CompiledItem>,
    pub warnings: Vec<CompileWarning>,
    panels: BTreeMap<(String, String), Vec<PanelAtTime>>,
}

#[derive(Clone, Debug, PartialEq)]
struct PanelAtTime {
    time: f64,
    id: String,
}

/// The shot, take, offset, and panel visible at a sampled frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample<'a> {
    pub shot: &'a str,
    pub take: &'a str,
    pub offset: u64,
    pub panel: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompileError {
    UnknownCut { cut: String },
    UnknownShot { shot: String },
    UnknownTake { shot: String, take: String },
    Cycle { cuts: Vec<String> },
    Limit { field: String, message: String },
    InvalidDuration { field: String, message: String },
    Arithmetic { field: String },
}

impl fmt::Display for CompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCut { cut } => write!(formatter, "unknown cut {cut}"),
            Self::UnknownShot { shot } => write!(formatter, "unknown shot {shot}"),
            Self::UnknownTake { shot, take } => {
                write!(formatter, "unknown take {take} for shot {shot}")
            }
            Self::Cycle { cuts } => write!(formatter, "cut cycle: {}", cuts.join(" -> ")),
            Self::Limit { message, .. } | Self::InvalidDuration { message, .. } => {
                formatter.write_str(message)
            }
            Self::Arithmetic { field } => write!(formatter, "frame arithmetic overflow at {field}"),
        }
    }
}
impl std::error::Error for CompileError {}

/// Compiles scenes and shots in document order using each shot's selected take.
pub fn compile_story(project: &Project) -> Result<Compiled, CompileError> {
    check_count_limits(project)?;
    preflight_story(project)?;
    let index = ProjectIndex::new(project);
    let context = CompileContext::new(project);
    let mut sequence = Sequence::empty();
    for scene in &project.scenes {
        for shot in &scene.shots {
            append_sequence(
                project,
                &mut sequence,
                shot_sequence(project, &index, &shot.id, None)?,
                "story",
            )?;
        }
    }
    Ok(context.finish(sequence))
}

/// Compiles a named cut, expanding nested cuts in place.
pub fn compile_cut(project: &Project, cut: &str) -> Result<Compiled, CompileError> {
    check_count_limits(project)?;
    let index = ProjectIndex::new(project);
    let analysis = analyze_cuts_with_index(project, &index);
    let cut_index = index
        .cut_index(cut)
        .ok_or_else(|| CompileError::UnknownCut { cut: cut.into() })?;
    analysis.check_index(cut_index)?;
    let mut context = CompileContext::new(project);
    let sequence = cut_sequence(project, &index, &mut context, cut)?;
    Ok(context.finish(sequence))
}

const MAX_TAKES_PER_SHOT: usize = 100;

pub(crate) fn check_count_limits(project: &Project) -> Result<(), CompileError> {
    if project.cuts.len() > MAX_CUTS {
        return Err(count_limit("cuts", MAX_CUTS));
    }
    if let Some((index, _)) = project
        .cuts
        .iter()
        .enumerate()
        .find(|(_, cut)| cut.clips.len() > MAX_CLIPS_PER_CUT)
    {
        return Err(count_limit(
            format!("cuts[{index}].clips"),
            MAX_CLIPS_PER_CUT,
        ));
    }
    let shots = project.scenes.iter().fold(0usize, |total, scene| {
        total.saturating_add(scene.shots.len())
    });
    if shots > MAX_SHOTS {
        return Err(count_limit("shots", MAX_SHOTS));
    }
    if project.scenes.len() > MAX_SCENES {
        return Err(count_limit("scenes", MAX_SCENES));
    }
    for (scene_index, scene) in project.scenes.iter().enumerate() {
        for (shot_index, shot) in scene.shots.iter().enumerate() {
            if shot.takes.len() > MAX_TAKES_PER_SHOT {
                return Err(count_limit(
                    format!("scenes[{scene_index}].shots[{shot_index}].takes"),
                    MAX_TAKES_PER_SHOT,
                ));
            }
        }
    }
    Ok(())
}

fn count_limit(field: impl Into<String>, maximum: usize) -> CompileError {
    CompileError::Limit {
        field: field.into(),
        message: format!("at most {maximum} items are allowed"),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ProjectIndex {
    cuts: HashMap<String, usize>,
    shots: HashMap<String, (usize, usize)>,
}

impl ProjectIndex {
    pub(crate) fn new(project: &Project) -> Self {
        let mut cuts = HashMap::with_capacity(project.cuts.len());
        for (index, cut) in project.cuts.iter().enumerate() {
            cuts.entry(cut.id.clone()).or_insert(index);
        }
        let shot_count = project.scenes.iter().map(|scene| scene.shots.len()).sum();
        let mut shots = HashMap::with_capacity(shot_count);
        for (scene_index, scene) in project.scenes.iter().enumerate() {
            for (shot_index, shot) in scene.shots.iter().enumerate() {
                shots
                    .entry(shot.id.clone())
                    .or_insert((scene_index, shot_index));
            }
        }
        Self { cuts, shots }
    }

    pub(crate) fn shot<'a>(&self, project: &'a Project, id: &str) -> Option<&'a crate::Shot> {
        self.shots
            .get(id)
            .map(|&(scene, shot)| &project.scenes[scene].shots[shot])
    }

    fn cut_index(&self, id: &str) -> Option<usize> {
        self.cuts.get(id).copied()
    }
}

#[derive(Clone, Debug)]
struct CutMetrics {
    height: usize,
    duration: u64,
    items: usize,
}

type SharedCycle = Arc<Vec<String>>;
type CutPostorder = (Vec<usize>, Vec<Option<SharedCycle>>);

/// Structural metrics for every cut, shared by compilation and validation.
///
/// The graph walk is deliberately iterative: invalid input must not consume the test
/// thread's stack merely to report its nesting limit. Only a sequence whose expanded
/// clip count is within 10000 is walked, so a walk is at most 10000 steps plus trims
/// arithmetic.
#[derive(Clone, Debug)]
pub(crate) struct CutAnalysis {
    results: Vec<Result<CutMetrics, SharedCompileError>>,
}

#[derive(Clone, Debug)]
enum SharedCompileError {
    Cycle(SharedCycle),
    Other(CompileError),
}

impl SharedCompileError {
    fn to_compile_error(&self) -> CompileError {
        match self {
            Self::Cycle(cuts) => CompileError::Cycle {
                cuts: cuts.as_ref().clone(),
            },
            Self::Other(error) => error.clone(),
        }
    }
}

impl CutAnalysis {
    pub(crate) fn check_index(&self, index: usize) -> Result<(), CompileError> {
        self.results[index]
            .as_ref()
            .map(|_| ())
            .map_err(SharedCompileError::to_compile_error)
    }

    pub(crate) fn validation_errors(&self) -> Vec<(usize, CompileError)> {
        let mut seen_cycles = HashSet::new();
        let mut errors = Vec::new();
        for (index, result) in self.results.iter().enumerate() {
            let Err(error) = result else {
                continue;
            };
            if let SharedCompileError::Cycle(cycle) = error {
                let identity = Arc::as_ptr(cycle) as usize;
                if !seen_cycles.insert(identity) {
                    continue;
                }
            }
            errors.push((index, error.to_compile_error()));
        }
        errors
    }
}

/// Analyzes all cuts in one iterative, memoized pass before any sequence is expanded.
#[cfg(test)]
pub(crate) fn analyze_cuts(project: &Project) -> CutAnalysis {
    let index = ProjectIndex::new(project);
    analyze_cuts_with_index(project, &index)
}

pub(crate) fn analyze_cuts_with_index(
    project: &Project,
    project_index: &ProjectIndex,
) -> CutAnalysis {
    let (order, cycles) = all_cut_postorder(project, &project_index.cuts);
    let mut results: Vec<Option<Result<CutMetrics, SharedCompileError>>> =
        vec![None; project.cuts.len()];

    for cut_index in order {
        analysis_step();
        if let Some(cuts) = &cycles[cut_index] {
            results[cut_index] = Some(Err(SharedCompileError::Cycle(Arc::clone(cuts))));
            continue;
        }
        let mut metrics = CutMetrics {
            height: 1,
            duration: 0,
            items: 0,
        };
        let mut error = None;
        for clip in &project.cuts[cut_index].clips {
            analysis_step();
            let source = match &clip.source {
                ClipSource::Shot(source) => {
                    shot_duration(project, project_index, &source.shot, source.take.as_deref())
                        .map_err(SharedCompileError::Other)
                }
                ClipSource::Cut(source) => match project_index.cuts.get(&source.cut) {
                    Some(&child) => match results[child]
                        .as_ref()
                        .expect("postorder measures nested cuts before their parent")
                    {
                        Ok(child) => Ok(Some((child.height, child.duration, child.items))),
                        Err(child_error) => Err(child_error.clone()),
                    },
                    None => Ok(None),
                },
            };
            match source {
                Ok(Some((height, source_duration, items))) => {
                    metrics.height = metrics.height.max(height.saturating_add(1));
                    metrics.items = saturating_item_add(metrics.items, items);
                    metrics.duration = metrics
                        .duration
                        .saturating_add(clip_duration(clip, source_duration));
                }
                Ok(None) => {}
                Err(source_error) => {
                    error.get_or_insert(source_error);
                }
            }
        }
        results[cut_index] = Some(match error {
            Some(error) => Err(error),
            None if metrics.height > MAX_CUT_NESTING => {
                Err(SharedCompileError::Other(nesting_limit()))
            }
            None if metrics.items > MAX_COMPILED_ITEMS => {
                Err(SharedCompileError::Other(item_limit()))
            }
            None => ensure_sequence_duration(
                project,
                metrics.duration,
                &format!("cut {}", project.cuts[cut_index].id),
            )
            .map(|()| metrics)
            .map_err(SharedCompileError::Other),
        });
    }

    CutAnalysis {
        results: results
            .into_iter()
            .map(|result| result.expect("every cut is visited by the all-cut traversal"))
            .collect(),
    }
}

pub(crate) fn preflight_story(project: &Project) -> Result<(), CompileError> {
    let mut items = 0usize;
    let mut duration = 0u64;
    for shot in project.scenes.iter().flat_map(|scene| &scene.shots) {
        let take_exists = shot.takes.iter().any(|take| take.id == shot.selected_take);
        if !take_exists {
            continue;
        }
        let shot_duration = duration_to_frames(
            project,
            shot.duration,
            &format!("shot {}.duration", shot.id),
        )?;
        duration = duration.saturating_add(shot_duration);
        if shot_duration != 0 {
            items = saturating_item_add(items, 1);
        }
    }
    if items > MAX_COMPILED_ITEMS {
        return Err(item_limit());
    }
    ensure_sequence_duration(project, duration, "story")
}

fn all_cut_postorder(project: &Project, indices: &HashMap<String, usize>) -> CutPostorder {
    #[derive(Clone, Copy)]
    struct Frame {
        cut: usize,
        next_clip: usize,
    }

    let mut states = vec![0u8; project.cuts.len()];
    let mut stack = Vec::new();
    let mut active = Vec::new();
    let mut positions = HashMap::new();
    let mut order = Vec::new();
    let mut cycles = vec![None; project.cuts.len()];

    for root in 0..project.cuts.len() {
        if states[root] != 0 {
            continue;
        }
        analysis_step();
        states[root] = 1;
        positions.insert(root, 0usize);
        active.push(root);
        stack.push(Frame {
            cut: root,
            next_clip: 0,
        });

        while let Some(frame) = stack.last_mut() {
            let cut = &project.cuts[frame.cut];
            if frame.next_clip == cut.clips.len() {
                let finished = frame.cut;
                stack.pop();
                active.pop();
                positions.remove(&finished);
                states[finished] = 2;
                order.push(finished);
                continue;
            }
            let clip = &cut.clips[frame.next_clip];
            frame.next_clip += 1;
            analysis_step();
            let ClipSource::Cut(source) = &clip.source else {
                continue;
            };
            let Some(&child) = indices.get(&source.cut) else {
                continue;
            };
            match states[child] {
                0 => {
                    states[child] = 1;
                    positions.insert(child, active.len());
                    active.push(child);
                    stack.push(Frame {
                        cut: child,
                        next_clip: 0,
                    });
                }
                1 => {
                    let start = positions[&child];
                    let mut cycle = active[start..]
                        .iter()
                        .map(|index| project.cuts[*index].id.clone())
                        .collect::<Vec<_>>();
                    cycle.push(project.cuts[child].id.clone());
                    let cycle = Arc::new(cycle);
                    for &member in &active[start..] {
                        if cycles[member].is_none() {
                            cycles[member] = Some(Arc::clone(&cycle));
                        }
                    }
                }
                2 => {}
                _ => unreachable!("cut visit state is one of three values"),
            }
        }
    }
    (order, cycles)
}

fn shot_duration(
    project: &Project,
    index: &ProjectIndex,
    shot_id: &str,
    requested_take: Option<&str>,
) -> Result<Option<(usize, u64, usize)>, CompileError> {
    let Some(shot) = index.shot(project, shot_id) else {
        return Ok(None);
    };
    let take = requested_take.unwrap_or(&shot.selected_take);
    if !shot.takes.iter().any(|candidate| candidate.id == take) {
        return Ok(None);
    }
    Ok(Some((
        0,
        duration_to_frames(
            project,
            shot.duration,
            &format!("shot {}.duration", shot.id),
        )?,
        1,
    )))
}

fn clip_duration(clip: &Clip, source_duration: u64) -> u64 {
    let requested_end = clip.out_frame.unwrap_or(source_duration);
    let input_start = clip.in_frame.min(source_duration);
    let mut input_end = requested_end.min(source_duration);
    if requested_end < clip.in_frame {
        input_end = input_start;
    }
    clip.slot.unwrap_or(input_end - input_start)
}

fn saturating_item_add(current: usize, addition: usize) -> usize {
    current
        .saturating_add(addition)
        .min(MAX_COMPILED_ITEMS.saturating_add(1))
}

fn nesting_limit() -> CompileError {
    CompileError::Limit {
        field: "nesting depth".into(),
        message: format!("cut nesting depth may not exceed {MAX_CUT_NESTING}"),
    }
}

fn item_limit() -> CompileError {
    CompileError::Limit {
        field: "compiled items".into(),
        message: format!("a sequence may not exceed {MAX_COMPILED_ITEMS} compiled items"),
    }
}

/// Compiles the active cut, or story order when no cut is active.
pub fn compile_active(project: &Project) -> Result<Compiled, CompileError> {
    match &project.active_cut {
        Some(cut) => compile_cut(project, cut),
        None => compile_story(project),
    }
}

/// Returns `None` for black padding or the end of a sequence.
pub fn sample(compiled: &Compiled, frame: u64) -> Option<Sample<'_>> {
    compiled.items.iter().find_map(|item| {
        (item.start <= frame && frame < item.end).then(|| {
            let offset = item.offset + (frame - item.start);
            let seconds = offset as f64 / f64::from(compiled.fps);
            let panel = compiled
                .panels
                .get(&(item.shot.clone(), item.take.clone()))
                .and_then(|panels| panels.iter().rev().find(|panel| panel.time <= seconds))
                .map(|panel| panel.id.as_str());
            Sample {
                shot: &item.shot,
                take: &item.take,
                offset,
                panel,
            }
        })
    })
}

#[derive(Clone, Debug)]
struct Sequence {
    duration: u64,
    items: Vec<CompiledItem>,
}
impl Sequence {
    fn empty() -> Self {
        Self {
            duration: 0,
            items: Vec::new(),
        }
    }
}

struct CompileContext {
    cache: HashMap<String, Sequence>,
    clipped_cache: HashMap<CutClipKey, PreparedSequence>,
    warnings: Vec<CompileWarning>,
    warning_keys: HashSet<(String, String)>,
    suppressed_warnings: usize,
    panels: BTreeMap<(String, String), Vec<PanelAtTime>>,
    fps: u32,
}

impl CompileContext {
    fn new(project: &Project) -> Self {
        let mut panels = BTreeMap::new();
        for scene in &project.scenes {
            for shot in &scene.shots {
                for take in &shot.takes {
                    panels.insert(
                        (shot.id.clone(), take.id.clone()),
                        take.panels
                            .iter()
                            .map(|panel| PanelAtTime {
                                time: panel.time,
                                id: panel.id.clone(),
                            })
                            .collect(),
                    );
                }
            }
        }
        Self {
            cache: HashMap::new(),
            clipped_cache: HashMap::new(),
            warnings: Vec::new(),
            warning_keys: HashSet::new(),
            suppressed_warnings: 0,
            panels,
            fps: project.fps,
        }
    }

    fn warn(&mut self, field: String, message: impl Into<String>) {
        let message = message.into();
        if !self.warning_keys.insert((field.clone(), message.clone())) {
            return;
        }
        if self.warnings.len() < MAX_COMPILE_WARNINGS {
            self.warnings.push(CompileWarning { field, message });
        } else {
            self.suppressed_warnings += 1;
        }
    }

    fn finish(mut self, sequence: Sequence) -> Compiled {
        if self.suppressed_warnings != 0 {
            self.warnings.push(CompileWarning {
                field: "compile.warnings".into(),
                message: format!(
                    "{} additional compile warnings were suppressed",
                    self.suppressed_warnings
                ),
            });
        }
        Compiled {
            fps: self.fps,
            frames: sequence.duration,
            items: sequence.items,
            warnings: self.warnings,
            panels: self.panels,
        }
    }
}

#[derive(Clone, Debug)]
struct PreparedSequence {
    source_duration: u64,
    sequence: Sequence,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct CutClipKey {
    cut: String,
    in_frame: u64,
    out_frame: Option<u64>,
    slot: Option<u64>,
}

fn cut_sequence(
    project: &Project,
    index: &ProjectIndex,
    context: &mut CompileContext,
    cut_id: &str,
) -> Result<Sequence, CompileError> {
    if let Some(sequence) = context.cache.get(cut_id) {
        return Ok(sequence.clone());
    }
    let cut = index
        .cuts
        .get(cut_id)
        .map(|&cut_index| &project.cuts[cut_index])
        .ok_or_else(|| CompileError::UnknownCut { cut: cut_id.into() })?;
    let result = (|| {
        let mut sequence = Sequence::empty();
        for clip in &cut.clips {
            let source = clip_sequence(project, index, context, clip)?;
            append_sequence(project, &mut sequence, source, &format!("cut {cut_id}"))?;
        }
        Ok(sequence)
    })();
    if let Ok(sequence) = &result {
        context.cache.insert(cut_id.into(), sequence.clone());
    }
    result
}

fn clip_sequence(
    project: &Project,
    index: &ProjectIndex,
    context: &mut CompileContext,
    clip: &Clip,
) -> Result<Sequence, CompileError> {
    let mut source = match &clip.source {
        ClipSource::Shot(source) => prepare_clip(
            project,
            context,
            shot_sequence(project, index, &source.shot, source.take.as_deref())?,
            clip,
        )?,
        ClipSource::Cut(source) => prepare_cut_clip(project, index, context, source, clip)?,
    };
    for item in &mut source.items {
        item.clip = Some(clip.id.clone());
    }
    Ok(source)
}

fn prepare_cut_clip(
    project: &Project,
    index: &ProjectIndex,
    context: &mut CompileContext,
    source: &crate::CutSource,
    clip: &Clip,
) -> Result<Sequence, CompileError> {
    let key = CutClipKey {
        cut: source.cut.clone(),
        in_frame: clip.in_frame,
        out_frame: clip.out_frame,
        slot: clip.slot,
    };
    if let Some(prepared) = context.clipped_cache.get(&key).cloned() {
        warn_clip_adjustments(context, clip, prepared.source_duration);
        return Ok(prepared.sequence.clone());
    }
    let source = cut_sequence(project, index, context, &source.cut)?;
    let source_duration = source.duration;
    let sequence = prepare_clip(project, context, source, clip)?;
    context.clipped_cache.insert(
        key,
        PreparedSequence {
            source_duration,
            sequence: sequence.clone(),
        },
    );
    Ok(sequence)
}

fn prepare_clip(
    project: &Project,
    context: &mut CompileContext,
    source: Sequence,
    clip: &Clip,
) -> Result<Sequence, CompileError> {
    warn_clip_adjustments(context, clip, source.duration);
    let mut sequence = trim_sequence(source, clip)?;
    slot_sequence(project, &mut sequence, clip)?;
    Ok(sequence)
}

fn warn_clip_adjustments(context: &mut CompileContext, clip: &Clip, source_duration: u64) {
    let requested_end = clip.out_frame.unwrap_or(source_duration);
    if clip.in_frame > source_duration {
        context.warn(
            format!("clip {}.in", clip.id),
            "in is beyond the source; the clip has no source frames",
        );
    }
    if requested_end > source_duration {
        context.warn(
            format!("clip {}.out", clip.id),
            "out is beyond the source and was clipped",
        );
    }
    if requested_end < clip.in_frame {
        context.warn(
            format!("clip {}.out", clip.id),
            "out precedes in; the clip has no source frames",
        );
    }
    if let Some(slot) = clip.slot {
        if slot < source_duration {
            context.warn(format!("clip {}.slot", clip.id), "slot cuts source content");
        } else if slot > source_duration {
            context.warn(
                format!("clip {}.slot", clip.id),
                "slot pads remaining frames with black",
            );
        }
    }
}

fn shot_sequence(
    project: &Project,
    index: &ProjectIndex,
    shot_id: &str,
    requested_take: Option<&str>,
) -> Result<Sequence, CompileError> {
    let shot = index
        .shot(project, shot_id)
        .ok_or_else(|| CompileError::UnknownShot {
            shot: shot_id.into(),
        })?;
    let take = requested_take.unwrap_or(&shot.selected_take);
    if !shot.takes.iter().any(|candidate| candidate.id == take) {
        return Err(CompileError::UnknownTake {
            shot: shot_id.into(),
            take: take.into(),
        });
    }
    let duration = duration_to_frames(project, shot.duration, &format!("shot {shot_id}.duration"))?;
    ensure_sequence_duration(project, duration, &format!("shot {shot_id}"))?;
    let mut sequence = Sequence::empty();
    if duration != 0 {
        push_item(
            &mut sequence.items,
            CompiledItem {
                start: 0,
                end: duration,
                shot: shot_id.into(),
                take: take.into(),
                offset: 0,
                clip: None,
            },
        )?;
    }
    sequence.duration = duration;
    Ok(sequence)
}

fn trim_sequence(mut source: Sequence, clip: &Clip) -> Result<Sequence, CompileError> {
    let requested_end = clip.out_frame.unwrap_or(source.duration);
    let start = clip.in_frame.min(source.duration);
    let mut end = requested_end.min(source.duration);
    if requested_end < clip.in_frame {
        end = start;
    }
    let mut trimmed = Sequence::empty();
    for item in source.items.drain(..) {
        let item_start = item.start.max(start);
        let item_end = item.end.min(end);
        if item_start >= item_end {
            continue;
        }
        let offset = item
            .offset
            .checked_add(item_start - item.start)
            .ok_or_else(|| CompileError::Arithmetic {
                field: format!("clip {} offset", clip.id),
            })?;
        push_item(
            &mut trimmed.items,
            CompiledItem {
                start: item_start - start,
                end: item_end - start,
                shot: item.shot,
                take: item.take,
                offset,
                clip: item.clip,
            },
        )?;
    }
    trimmed.duration = end - start;
    Ok(trimmed)
}

fn slot_sequence(
    project: &Project,
    source: &mut Sequence,
    clip: &Clip,
) -> Result<(), CompileError> {
    let Some(slot) = clip.slot else {
        return Ok(());
    };
    ensure_sequence_duration(project, slot, &format!("clip {}.slot", clip.id))?;
    if slot < source.duration {
        for item in &mut source.items {
            item.end = item.end.min(slot);
        }
        source.items.retain(|item| item.start < item.end);
    }
    source.duration = slot;
    Ok(())
}

fn append_sequence(
    project: &Project,
    target: &mut Sequence,
    mut source: Sequence,
    field: &str,
) -> Result<(), CompileError> {
    let start = target.duration;
    let duration = start
        .checked_add(source.duration)
        .ok_or_else(|| CompileError::Arithmetic {
            field: field.into(),
        })?;
    ensure_sequence_duration(project, duration, field)?;
    for mut item in source.items.drain(..) {
        item.start = start
            .checked_add(item.start)
            .ok_or_else(|| CompileError::Arithmetic {
                field: field.into(),
            })?;
        item.end = start
            .checked_add(item.end)
            .ok_or_else(|| CompileError::Arithmetic {
                field: field.into(),
            })?;
        push_item(&mut target.items, item)?;
    }
    target.duration = duration;
    Ok(())
}

fn push_item(items: &mut Vec<CompiledItem>, item: CompiledItem) -> Result<(), CompileError> {
    if items.len() == items.capacity() {
        let additional = (items.capacity().max(1)).min(MAX_COMPILED_ITEMS - items.len());
        items.reserve_exact(additional);
    }
    items.push(item);
    Ok(())
}

fn duration_to_frames(project: &Project, duration: f64, field: &str) -> Result<u64, CompileError> {
    if !duration.is_finite() || duration < 0.0 {
        return Err(CompileError::InvalidDuration {
            field: field.into(),
            message: format!("{field} must be a finite non-negative duration"),
        });
    }
    let frames = duration * f64::from(project.fps);
    if !frames.is_finite() || frames.round() > u64::MAX as f64 {
        return Err(CompileError::InvalidDuration {
            field: field.into(),
            message: format!("{field} cannot be represented as frames"),
        });
    }
    Ok(frames.round() as u64)
}

fn ensure_sequence_duration(
    project: &Project,
    duration: u64,
    field: &str,
) -> Result<(), CompileError> {
    let maximum = MAX_SEQUENCE_SECONDS
        .checked_mul(u64::from(project.fps))
        .ok_or_else(|| CompileError::Arithmetic {
            field: field.into(),
        })?;
    if duration > maximum {
        return Err(CompileError::Limit {
            field: "sequence duration".into(),
            message: "a sequence may not exceed 10 hours".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cut, CutSource, ExtraFields, ShotSource};

    fn shot_clip(id: String) -> Clip {
        Clip {
            id,
            source: ClipSource::Shot(ShotSource {
                shot: "shot-1".into(),
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

    fn cut_clip(id: String, cut: String) -> Clip {
        Clip {
            id,
            source: ClipSource::Cut(CutSource {
                cut,
                extra: ExtraFields::default(),
                source_extra: ExtraFields::default(),
            }),
            in_frame: 0,
            out_frame: None,
            slot: None,
            extra: ExtraFields::default(),
        }
    }

    fn replace_shots(project: &mut Project, count: usize) {
        let template = project.scenes[0].shots[0].clone();
        project.scenes.truncate(1);
        project.scenes[0].shots = (0..count)
            .map(|index| {
                let mut shot = template.clone();
                shot.id = format!("bulk-shot-{index}");
                shot.selected_take = format!("bulk-take-{index}");
                shot.takes.truncate(1);
                shot.takes[0].id = shot.selected_take.clone();
                shot
            })
            .collect();
    }

    #[test]
    fn ten_thousand_cut_cycle_is_rejected_before_analysis() {
        let mut project: Project =
            serde_json::from_str(include_str!("../tests/fixtures/small.json"))
                .expect("fixture is a project");
        project.cuts = (0..10_000)
            .map(|index| {
                let next = (index + 1) % 10_000;
                Cut {
                    id: format!("cycle-{index}"),
                    name: String::new(),
                    clips: vec![cut_clip(
                        format!("cycle-clip-{index}"),
                        format!("cycle-{next}"),
                    )],
                    extra: ExtraFields::default(),
                }
            })
            .collect();

        reset_analysis_steps();
        let _guard = limit_analysis_steps(0);
        assert!(matches!(
            compile_cut(&project, "cycle-0"),
            Err(CompileError::Limit { ref field, .. }) if field == "cuts"
        ));
        assert_eq!(analysis_steps(), 0);
    }

    #[test]
    fn one_hundred_thousand_clips_are_rejected_before_analysis() {
        let mut project: Project =
            serde_json::from_str(include_str!("../tests/fixtures/small.json"))
                .expect("fixture is a project");
        project.cuts = vec![Cut {
            id: "clip-limit".into(),
            name: String::new(),
            clips: (0..100_000)
                .map(|index| cut_clip(format!("bulk-clip-{index}"), format!("target-{index}")))
                .collect(),
            extra: ExtraFields::default(),
        }];

        reset_analysis_steps();
        let _guard = limit_analysis_steps(0);
        assert!(matches!(
            compile_cut(&project, "clip-limit"),
            Err(CompileError::Limit { ref field, .. }) if field == "cuts[0].clips"
        ));
        assert_eq!(analysis_steps(), 0);
    }

    #[test]
    fn ten_thousand_clips_over_fifty_thousand_shots_are_rejected_before_analysis() {
        let mut project: Project =
            serde_json::from_str(include_str!("../tests/fixtures/small.json"))
                .expect("fixture is a project");
        replace_shots(&mut project, 50_000);
        project.cuts = (0..10)
            .map(|cut_index| Cut {
                id: format!("bulk-cut-{cut_index}"),
                name: String::new(),
                clips: (0..1000)
                    .map(|clip_index| {
                        let index = cut_index * 1000 + clip_index;
                        shot_clip(format!("bulk-clip-{index}"))
                    })
                    .collect(),
                extra: ExtraFields::default(),
            })
            .collect();

        reset_analysis_steps();
        let _guard = limit_analysis_steps(0);
        assert!(matches!(
            compile_cut(&project, "bulk-cut-0"),
            Err(CompileError::Limit { ref field, .. }) if field == "shots"
        ));
        assert_eq!(analysis_steps(), 0);
    }

    #[test]
    fn validate_rejects_thirty_thousand_cut_chain_without_analysis() {
        let mut project: Project =
            serde_json::from_str(include_str!("../tests/fixtures/small.json"))
                .expect("fixture is a project");
        project.cuts = (0..30_000)
            .map(|index| Cut {
                id: format!("validate-chain-{index}"),
                name: String::new(),
                clips: vec![cut_clip(
                    format!("validate-chain-clip-{index}"),
                    format!("validate-chain-{}", (index + 1) % 30_000),
                )],
                extra: ExtraFields::default(),
            })
            .collect();

        reset_analysis_steps();
        let _guard = limit_analysis_steps(0);
        let issues = crate::validate(&project);
        assert!(issues.iter().any(|issue| issue.field == "cuts"));
        assert_eq!(analysis_steps(), 0);
    }

    #[test]
    fn one_hundred_cut_cycle_shares_one_report() {
        let mut project: Project =
            serde_json::from_str(include_str!("../tests/fixtures/small.json"))
                .expect("fixture is a project");
        project.cuts = (0..100)
            .map(|index| Cut {
                id: format!("shared-cycle-{index}"),
                name: String::new(),
                clips: vec![cut_clip(
                    format!("shared-cycle-clip-{index}"),
                    format!("shared-cycle-{}", (index + 1) % 100),
                )],
                extra: ExtraFields::default(),
            })
            .collect();
        let indices = project
            .cuts
            .iter()
            .enumerate()
            .map(|(index, cut)| (cut.id.clone(), index))
            .collect();

        let (_, cycles) = all_cut_postorder(&project, &indices);
        let first = cycles[0].as_ref().expect("cycle report").as_ptr();
        assert!(cycles.iter().all(|cycle| {
            cycle
                .as_ref()
                .is_some_and(|report| report.as_ptr() == first)
        }));

        let issues = crate::validate(&project);
        assert_eq!(
            issues
                .iter()
                .filter(|issue| issue.message.contains("cut cycle"))
                .count(),
            1,
            "one cycle must produce one validation issue: {issues:?}"
        );
    }

    #[test]
    fn one_hundred_cuts_of_one_thousand_clips_analyze_once() {
        let mut project: Project =
            serde_json::from_str(include_str!("../tests/fixtures/small.json"))
                .expect("fixture is a project");
        project.cuts = (0..100)
            .map(|cut_index| Cut {
                id: format!("legal-cut-{cut_index}"),
                name: String::new(),
                clips: (0..1000)
                    .map(|clip_index| shot_clip(format!("legal-clip-{cut_index}-{clip_index}")))
                    .collect(),
                extra: ExtraFields::default(),
            })
            .collect();

        reset_analysis_steps();
        let analysis = analyze_cuts(&project);
        assert!((0..100).all(|index| analysis.check_index(index).is_ok()));
        assert!(
            analysis_steps() <= 200_200,
            "analysis took {} steps",
            analysis_steps()
        );
    }

    #[test]
    fn cut_analysis_steps_are_linear_for_a_thirty_thousand_cut_chain() {
        let mut project: Project =
            serde_json::from_str(include_str!("../tests/fixtures/small.json"))
                .expect("fixture is a project");
        project.cuts = (0..30_000)
            .map(|index| Cut {
                id: format!("chain-{index}"),
                name: String::new(),
                clips: vec![if index == 29_999 {
                    shot_clip(format!("clip-{index}"))
                } else {
                    cut_clip(format!("clip-{index}"), format!("chain-{}", index + 1))
                }],
                extra: ExtraFields::default(),
            })
            .collect();

        reset_analysis_steps();
        let analysis = analyze_cuts(&project);
        assert!(matches!(
            analysis.check_index(0),
            Err(CompileError::Limit { ref field, .. }) if field == "nesting depth"
        ));
        let nodes = project.cuts.len();
        let clips = project
            .cuts
            .iter()
            .map(|cut| cut.clips.len())
            .sum::<usize>();
        assert!(
            analysis_steps() <= 2 * (nodes + clips),
            "analysis used {} steps for {nodes} cuts and {clips} clips",
            analysis_steps()
        );
    }
}
