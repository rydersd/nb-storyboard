# S1 30-scene edit and render evidence

Measured 2026-11-07 on an Apple M4 with 24 GiB memory.

- Build: Cargo release profile, all features
- Fixture: 30 scenes with one shot, take, and blank panel per scene
- Operation: add one text layer at sequence position 1, commit it, then render that panel in board mode at 960 px width
- Result: **37.842625 ms** for add-text plus board render (one measured run)
- Budget: 2 seconds
- Outcome: pass

Command:

```sh
cargo test --release -p storyboard-cli --all-features --test cli cli_thirty_scene_add_text_and_board_render_stays_under_two_seconds -- --exact --nocapture
```

The machine was under substantial concurrent load immediately before the run: load averages were 12.11, 16.90, and 22.65. Cargo's initial release compilation took 1 minute 31 seconds and is excluded from the timed edit-and-render interval inside the test.
