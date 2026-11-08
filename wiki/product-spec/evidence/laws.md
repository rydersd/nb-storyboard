# Law mutation evidence

Task 8-fix4 re-ran the full mutation table against the recursive project
generator, including the rebase-order and image-decoding gaps identified in
the fix-round brief. Each mutation was applied locally, checked by the named
law, and reverted before the next row. `PROPTEST_RNG_SEED` values identify the
reproduction run; deterministic cases do not need a seed.

The shared generator produces 1–5 scenes, 1–4 shots per scene, 1–3 takes per
shot, 1–3 panels per take, and 0–4 layers selected from image, text, solid, and
box kinds. It adds stickies pinned at every supported level plus replies, 0–3
acyclic cuts whose clips use shot and earlier-cut sources, unknown fields, and
random ids. Store laws consume the complete topology. Operation laws project
the generated document to the first scene/shot/take/panel and retain root and
reply stickies before generating setup plus every operation kind, including
nested batches. The drawable law similarly retains the first panel context,
ensures a second panel, and injects image layers with no source, a missing
source, random bytes named `.png`, and a truncated real PNG. These projections
remove irrelevant render and inverse work without narrowing the behavior under
test.

Every property runs exactly 18 generated cases with failure persistence
disabled, so the configured count is the actual count. With no other Cargo or
Rust build running, `cargo test --workspace --all-features law_` took 38.93 s
wall time: core 5.21 s and store 19.62 s (24.83 s test time). This is below the
60 s debug test-time ceiling.

| # | Law / test | Mutation | Failing case | Result |
| --- | --- | --- | --- | --- |
| 1 | Inverse — `law_inverse` | Restoring a removed scene drops its nested shots. | Seed `801`; digest `5f75c8c5bb82b3d56a8d1f6b451de8aad9da360be1a882bd2d25abeaeaf54d55`. The generated `remove_scene` case contained a nested shot, which disappeared after inverse application. | killed |
| 2 | Validity — `law_validity_commit_rejects_invalid_result` | Remove result validation from `apply`. | Deterministic invalid `AddScene { id: "bad/id" }` landed. | killed |
| 3 | Drawable — `law_drawable` | Panic when an image source is missing. | Seed `803`; digest `dc9618c71b063843628f4d299b143add38f19df953881636f8644200b74879b2`. A generated missing image source panicked instead of producing a warning. | killed |
| 4 | Drawable — `law_drawable` | Only find the first panel of each take. | Seed `804`; digest `3235ac9cc6ed918353ba647b66bd554da12d18f0dbe151df01b7e333d92f9d01`. A generated second panel was rejected as unknown. | killed |
| 5 | No lost work — `law_no_lost_work_external_edit_has_real_footprints` | Give external edits an empty footprint. | Deterministic overlapping stale scene edit landed instead of conflicting. | killed |
| 6 | Order independence — `law_order_independence` | Make `disjoint` always return true. | Seed `806`; digest `4a7a346d40aa6b30eb0afb8d1722aac30905fae9cdef0ee3d1f4e6c2ed20a148`. Two writes to the generated scene title were classified as disjoint. | killed |
| 7 | Unbroken history — `law_unbroken_history_recovers_each_crash_point` | Make pending-commit recovery a no-op. | Deterministic `after_rename` crash lost the landed commit's original author. | killed |
| 8 | Honest authorship — `law_honest_authorship` | Omit `ReplySticky` from `sticky_authors`. | Seed `808`, `other_agent = true`; an agent reply landed under another author. | killed |
| 9 | Containment — `law_containment` | Let destination creation follow symlinks. | Seed `809`, segment `pD8iTlQ`; import wrote through a generated `sources/` symlink. | killed |
| 10 | Determinism — `law_determinism` | Collect story shots in a `HashSet` before compilation. | Seed `810`; digest `86a5e9cf87804c3750b845b5a79151429f1c9efecb52cb896952f0458599d248`. Repeated compilation returned different item orders. | killed |
| 11 | Format round trip — `law_round_trip_keeps_sticky_unknown_fields` | Mark `Sticky.extra` with `serde(skip)`. | Deterministic `future_direct_sticky = {"nested":[1,2,3]}` became null/missing. | killed |
| 12 | Determinism — `law_determinism_cyclic_compile_terminates` | Spin while compiling an active cyclic cut. | Deterministic worker exceeded the two-second deadline and was killed by the parent. | killed |
| 13 | Containment — `law_containment_cache_put_creates_missing_cache` | Require `.cache` to exist before `cache_put`. | Deterministic fresh directory failed with `No such file or directory`. | killed |
| 14 | No lost work — `law_store_sequences_commit_undo_and_concurrent` | Find the first matching history version instead of the newest. | Seed `814`; generated commit/undo history falsely conflicted on a disjoint concurrent commit. | killed |
| 15 | No lost work — single- and multi-step undo overlap laws | Remove the undo overlap guard. | Seed `815`; after Bob undid his shot edit, both one-step and two-step Alice undo erased Carol's later scene title. | killed |
| 16 | No lost work — `law_no_lost_work_checks_every_intervening_commit` | Check only the newest intervening commit during stale rebase. | Seed `816`; four intervening commits, overlap selector `8535313051937429936`. An earlier overlap was missed and the stale write landed. | killed |
| 17 | No lost work — single- and multi-step undo overlap laws | Let undo select any author's commit. | Seed `818`; Alice's undo selected and erased Carol's later scene edit. | killed |
| 18 | No lost work — `law_no_lost_work_external_edit_has_real_footprints` | Skip hand-edit records while collecting intervening footprints. | Deterministic overlapping stale scene edit landed over the hand edit. | killed |
| 19 | No lost work — store law suite | Treat an unknown base as an empty intervening history. | The following history-reach check still returns a conflict because an empty record list cannot end at the on-disk version. The complete store law binary passed 13 tests with 1 helper ignored. | equivalent |
| 20 | No lost work — single- and multi-step undo overlap laws | Let an undo cancellation match any earlier non-undo record by replacing the target-version check with `(true || ...)`. | Seed `817`; `alice_title = "C8e165U2x59"`, `bob_title = "pMMD"`, `carol_title = "NfjPOZ7"`. Alice's undo landed and erased Carol's title after Bob undid his disjoint shot edit. | killed |
| 21 | No lost work — `law_no_lost_work` | Always exempt `scene/order` and `ids/scene` footprints during stale rebase. | Seed `841`; Alice's stale indexed `AddScene` landed over Bob's indexed scene insertion. The same law also generates concurrent `MoveScene`, `MoveShot`, and indexed `AddShot` cases. | killed |
| 22 | Drawable — `law_drawable` | Panic when an image layer has no `src`. | Seed `842`; the generated no-source image panicked instead of returning `Ok` with a warning. | killed |
| 23 | Drawable — `law_drawable` | Panic when an image file cannot be decoded. | Seed `843`; random bytes named `.png` panicked instead of returning `Ok` with a warning. The law also supplies a truncated real PNG. | killed |

All 22 non-equivalent mutations are direct law failures at the 18-case
configuration. Row 19 remains equivalent because the following history-reach
check independently rejects the unknown base; asserting a particular conflict
wording would exceed the spec without changing behavior. The mutation run left
no production changes behind.
