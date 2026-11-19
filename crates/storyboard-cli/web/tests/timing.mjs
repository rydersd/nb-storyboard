import assert from 'node:assert/strict';
import test from 'node:test';

import { clampMediaTime, contentDuration, durationWithReplacedAudio, shouldSyncMedia } from '../js/core/timing.js';

test('a held source range seeks inside its last frame rather than one frame early', () => {
  const end = 5;
  const held = clampMediaTime(0, end, 8);
  assert.ok(held >= end - 1 / 30, `30 fps last frame begins at ${end - 1 / 30}, got ${held}`);
  assert.ok(held < end, 'seeking to the exact media end can trigger ended');
  assert.equal(clampMediaTime(2, 4, 1), 2);
  assert.equal(clampMediaTime(2, 4, 3), 3);
});

test('a frame step or hard cut seeks media even below the normal drift threshold', () => {
  assert.equal(shouldSyncMedia(0, 2 / 30, 30, false), false);
  assert.equal(shouldSyncMedia(0, 2 / 30, 30, true), true);
  assert.equal(shouldSyncMedia(1, 1 + 1 / 30, 30, true), true);
  assert.equal(shouldSyncMedia(1, 1 + 0.005, 30, true), false);
  assert.equal(shouldSyncMedia(1, 1.15, 30, false), true);
});

test('media edits retain the last storyboard panel and honor the longer attachment', () => {
  const shot = {
    source_movie: { start: 2, end: 6 },
    source_audio: { start: 0, end: 5 },
    takes: [{ panels: [{ time: 0 }, { time: 3 }] }],
  };
  assert.equal(contentDuration(shot, 30), 5);
  assert.equal(contentDuration(shot, 30, { start: 2, end: 2.5 }, null), 3 + 1 / 30);
  assert.equal(contentDuration(shot, 30, null, { start: 1, end: 2 }), 3 + 1 / 30);
  assert.equal(contentDuration(shot, 30, null, null), 3 + 1 / 30);
});

test('an empty board still has one project frame after both media are removed', () => {
  assert.equal(contentDuration({ takes: [] }, 24, null, null), 1 / 24);
});

test('replacing a derived voice-over shortens the shot but preserves a manual hold', () => {
  const shot = { duration: 8, source_audio: { start: 0, end: 8 }, takes: [{ panels: [{ time: 0 }] }] };
  const shorter = { start: 0, end: 3 };
  assert.equal(durationWithReplacedAudio(shot, 24, shorter), 3);
  assert.equal(durationWithReplacedAudio({ ...shot, duration: 10 }, 24, shorter), 10);
  assert.equal(durationWithReplacedAudio({ ...shot, source_audio: null, duration: 2 }, 24, shorter), 3);
  assert.equal(durationWithReplacedAudio({ ...shot, takes: [{ panels: [{ time: 4 }] }] }, 24, shorter), 4 + 1 / 24);
});
