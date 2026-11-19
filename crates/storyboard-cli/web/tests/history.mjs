import assert from 'node:assert/strict';
import test from 'node:test';

import { historyShotId } from '../js/core/history-map.js';

const doc = { shots: [{ id: 'shot-12', title: 'Arrival' }, { id: 'shot-2', title: 'Exit' }] };

test('history mapping selects the explicitly named current shot', () => {
  assert.equal(historyShotId({ summary: 'updated shot-12 layer title' }, doc), 'shot-12');
  assert.equal(historyShotId({ summary: 'updated the Arrival board' }, doc), 'shot-12');
});

test('history mapping does not match a shot id embedded in a longer token', () => {
  assert.equal(historyShotId({ summary: 'updated shot-120 layer title' }, doc), null);
});

test('history mapping uses exact shot, panel, take and layer footprints before summary fallback', () => {
  assert.equal(historyShotId({ summary: 'updated notes', footprints: ['shot/shot-12/notes'] }, doc), 'shot-12');
  assert.equal(historyShotId({ summary: 'updated notes', footprints: ['take/shot-2/take-b'] }, doc), 'shot-2');
  assert.equal(historyShotId({ summary: 'updated notes', footprints: ['panel/shot-12/take-a/panel-a'] }, doc), 'shot-12');
  assert.equal(historyShotId({ summary: 'updated notes', footprints: ['layer/shot-2/take-a/panel-a/layer-a'] }, doc), 'shot-2');
});
