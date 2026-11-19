import assert from 'node:assert/strict';
import test from 'node:test';
import { panelInsertionOps, panelRetimeOps } from '../js/core/panel-insert.js';

test('inserting between adjacent boards shifts later boundaries by one frame', () => {
  const shot = { id: 'shot-1', duration: 2 };
  const take = { id: 'take-1', panels: [
    { id: 'first', time: 0 }, { id: 'second', time: 1 / 24 }, { id: 'third', time: 2 / 24 },
  ] };
  assert.deepEqual(panelInsertionOps(shot, take, 1, 24), [
    { update_panel: { shot: 'shot-1', take: 'take-1', panel: 'second', patch: { time: 2 / 24 } } },
    { update_panel: { shot: 'shot-1', take: 'take-1', panel: 'third', patch: { time: 3 / 24 } } },
    { add_panel: { shot: 'shot-1', take: 'take-1', index: 1, time: 1 / 24 } },
  ]);
});

test('an appended board extends a full shot for its displayed frame', () => {
  const shot = { id: 'shot-1', duration: 2 / 24 };
  const take = { id: 'take-1', panels: [
    { id: 'first', time: 0 }, { id: 'second', time: 1 / 24 },
  ] };
  assert.deepEqual(panelInsertionOps(shot, take, 2, 24), [
    { update_shot: { shot: 'shot-1', patch: { duration: 3 / 24 } } },
    { add_panel: { shot: 'shot-1', take: 'take-1', index: 2, time: 2 / 24 } },
  ]);
});

test('retiming a board past another moves its strip position and extends the shot', () => {
  const shot = { id: 'shot-1', duration: 3 / 24 };
  const take = { id: 'take-1', panels: [
    { id: 'first', time: 0 }, { id: 'second', time: 1 / 24 }, { id: 'third', time: 2 / 24 },
  ] };
  shot.takes = [take];
  const ops = panelRetimeOps(shot, take, 'first', 0.167, 24);
  assert.ok(Math.abs(ops[0].update_shot.patch.duration - 5 / 24) < 1e-8);
  assert.deepEqual(ops.slice(1), [
    { update_panel: { shot: 'shot-1', take: 'take-1', panel: 'first', patch: { time: 4 / 24 } } },
    { move_panel: { shot: 'shot-1', take: 'take-1', panel: 'second', index: 0 } },
    { move_panel: { shot: 'shot-1', take: 'take-1', panel: 'third', index: 1 } },
  ]);
});

test('insertion grows the shot before shifting an end panel', () => {
  const shot = { id: 'shot-1', duration: 2 / 24 };
  const take = { id: 'take-1', panels: [
    { id: 'first', time: 0 }, { id: 'last', time: 1 / 24 },
  ] };
  assert.deepEqual(panelInsertionOps(shot, take, 1, 24), [
    { update_shot: { shot: 'shot-1', patch: { duration: 3 / 24 } } },
    { update_panel: { shot: 'shot-1', take: 'take-1', panel: 'last', patch: { time: 2 / 24 } } },
    { add_panel: { shot: 'shot-1', take: 'take-1', index: 1, time: 1 / 24 } },
  ]);
});

test('retiming snaps to the project frame and rejects an occupied frame', () => {
  const shot = { id: 'shot-1', duration: 2 };
  const take = { id: 'take-1', panels: [
    { id: 'first', time: 0 }, { id: 'second', time: 1 / 24 },
  ] };
  shot.takes = [take];
  assert.deepEqual(panelRetimeOps(shot, take, 'second', 0.084, 24), [
    { update_panel: { shot: 'shot-1', take: 'take-1', panel: 'second', patch: { time: 2 / 24 } } },
  ]);
  assert.throws(() => panelRetimeOps(shot, take, 'first', 0.042, 24), /already starts/);
});

test('moving the last board earlier shortens a content-sized shot', () => {
  const take = { id: 'take-1', panels: [
    { id: 'first', time: 0 }, { id: 'last', time: 4 / 24 },
  ] };
  const shot = { id: 'shot-1', duration: 5 / 24, takes: [take] };
  assert.deepEqual(panelRetimeOps(shot, take, 'last', 2 / 24, 24), [
    { update_panel: { shot: 'shot-1', take: 'take-1', panel: 'last', patch: { time: 2 / 24 } } },
    { update_shot: { shot: 'shot-1', patch: { duration: 3 / 24 } } },
  ]);
});

test('moving the last board earlier preserves an explicit longer hold and attached media', () => {
  const take = { id: 'take-1', panels: [
    { id: 'first', time: 0 }, { id: 'last', time: 4 / 24 },
  ] };
  const held = { id: 'shot-1', duration: 1, takes: [take] };
  const withMovie = { id: 'shot-1', duration: 1, takes: [take],
    source_movie: { src: 'movie.mp4', start: 0, end: 1 } };
  const expected = [
    { update_panel: { shot: 'shot-1', take: 'take-1', panel: 'last', patch: { time: 2 / 24 } } },
  ];
  assert.deepEqual(panelRetimeOps(held, take, 'last', 2 / 24, 24), expected);
  assert.deepEqual(panelRetimeOps(withMovie, take, 'last', 2 / 24, 24), expected);
});
