use serde_json::{Value, json};
use storyboard_core::{ClipSource, CutSource, Op, ShotSource};
use storyboard_store::{import_source, scan_sources};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    args::*,
    ops_api::{self, ApiError},
};

pub fn run(command: Command) -> Result<Value, ApiError> {
    match command {
        Command::Mcp(_) => Err(ops_api::validation(
            "MCP must run through the stdio entry point",
        )),
        Command::New(a) => ops_api::create_project(&a.dir, &a.title, a.fps, a.width, a.height),
        Command::AddScene(a) => write(
            &a.write,
            Op::AddScene {
                id: a.id,
                title: a.title,
                index: a.index,
            },
        ),
        Command::AddShot(a) => write(
            &a.write,
            Op::AddShot {
                scene: a.scene,
                id: a.id,
                title: a.title,
                index: a.index,
            },
        ),
        Command::AddPanel(a) => {
            let op = ops_api::add_panel_op(
                &a.write.dir,
                a.shot,
                a.take,
                a.at.as_deref(),
                a.id,
                a.time,
                a.index,
            )?;
            write(&a.write, op)
        }
        Command::AddText(a) => {
            let op = ops_api::add_text_op(
                &a.write.dir,
                a.shot,
                a.take,
                a.panel,
                a.at.as_deref(),
                a.text,
                a.x,
                a.y,
                a.width,
                a.height,
                a.font_size,
                a.color,
            )?;
            write(&a.write, op)
        }
        Command::AddImage(a) => {
            let op = ops_api::add_image_op(
                &a.write.dir,
                a.shot,
                a.take,
                a.panel,
                a.at.as_deref(),
                &a.src,
                a.x,
                a.y,
                a.width,
                a.height,
                a.fit,
            )?;
            write(&a.write, op)
        }
        Command::Import(a) => Ok(
            json!({"path": import_source(&a.dir, &a.source, &std::path::PathBuf::from("sources").join(a.destination))?}),
        ),
        Command::Sources(a) => Ok(json!({"sources": scan_sources(&a.dir)?})),
        Command::MoveShot(a) => write(
            &a.write,
            Op::MoveShotBetweenScenes {
                shot: a.shot,
                scene: a.scene,
                index: a.index,
            },
        ),
        Command::AddTake(a) => write(
            &a.write,
            Op::AddTake {
                shot: a.shot,
                id: a.id,
                name: a.name,
                index: a.index,
            },
        ),
        Command::AddCut(a) => write(
            &a.write,
            Op::AddCut {
                id: a.id,
                name: a.name,
                index: a.index,
            },
        ),
        Command::UpdateCut(a) => write(
            &a.write,
            Op::UpdateCut {
                cut: a.cut,
                patch: [("name".into(), json!(a.name))].into_iter().collect(),
            },
        ),
        Command::RemoveCut(a) => write(&a.write, Op::RemoveCut { cut: a.cut }),
        Command::AddClip(a) => {
            let source = match (a.shot, a.take, a.nested_cut) {
                (Some(shot), take, None) => ClipSource::Shot(ShotSource {
                    shot,
                    take,
                    extra: Default::default(),
                    source_extra: Default::default(),
                }),
                (None, None, Some(cut)) => ClipSource::Cut(CutSource {
                    cut,
                    extra: Default::default(),
                    source_extra: Default::default(),
                }),
                _ => {
                    return Err(ops_api::validation(
                        "provide --shot [--take] or --nested-cut",
                    ));
                }
            };
            write(
                &a.write,
                Op::AddClip {
                    cut: a.cut,
                    id: a.id,
                    source,
                    index: a.index,
                },
            )
        }
        Command::UpdateClip(a) => {
            let op = ops_api::update_clip_take_op(&a.write.dir, a.cut, a.clip, a.take)?;
            write(&a.write, op)
        }
        Command::RemoveClip(a) => write(
            &a.write,
            Op::RemoveClip {
                cut: a.cut,
                clip: a.clip,
            },
        ),
        Command::SetShot(a) => {
            let op = ops_api::set_shot_op(
                a.shot, a.title, a.action, a.dialogue, a.speaker, a.camera, a.notes, a.duration,
            )?;
            write(&a.write, op)
        }
        Command::SelectTake(a) => write(
            &a.write,
            Op::SelectTake {
                shot: a.shot,
                take: a.take,
            },
        ),
        Command::Trim(a) => {
            let op = ops_api::trim_op(a.cut, a.clip, a.r#in, a.out, a.slot)?;
            write(&a.write, op)
        }
        Command::Undo(a) => ops_api::undo_ops(
            &a.write.dir,
            &a.write.base_version,
            ops_api::author(a.write.author)?,
            a.steps,
        ),
        Command::History(a) => ops_api::history_result(&a.dir, a.limit, a.full),
        Command::Show(a) => ops_api::show(&a.dir, a.cut.as_deref()),
        Command::PanelImage(a) => ops_api::panel_image(a),
        Command::CutSheet(a) => ops_api::cut_sheet(a),
        Command::Note(a) => write(
            &a.write,
            Op::AddSticky {
                id: None,
                text: a.text,
                kind: a.kind,
                author: ops_api::author(a.write.author.clone())?,
                pin: ops_api::parse_pin(&a.pin)?,
                created: now()?,
            },
        ),
        Command::Reply(a) => write(
            &a.write,
            Op::ReplySticky {
                sticky: a.sticky,
                id: None,
                text: a.text,
                kind: a.kind,
                author: ops_api::author(a.write.author.clone())?,
                created: now()?,
            },
        ),
        Command::Resolve(a) => write(&a.write, Op::ResolveSticky { sticky: a.sticky }),
        Command::Notes(a) => {
            ops_api::notes_result(&a.dir, a.kind, a.author, a.pin, a.resolved, a.unresolved)
        }
        Command::Schema(a) => ops_api::schema(a.op.as_deref()),
    }
}

fn write(write: &WriteArgs, op: Op) -> Result<Value, ApiError> {
    ops_api::commit_ops(
        &write.dir,
        &write.base_version,
        vec![op],
        ops_api::author(write.author.clone())?,
        write.note.clone(),
    )
}

fn now() -> Result<String, ApiError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| ops_api::validation(error.to_string()))
}
