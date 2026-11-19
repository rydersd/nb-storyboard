import assert from 'node:assert/strict';
import test from 'node:test';

import { revisionBrief } from '../js/core/revision-brief.js';

test('new project without cuts produces a usable Claude brief', () => {
  const doc = {
    title: 'Revision', fps: 24, format: { width: 1920, height: 1080 },
    scenes: [{ id: 'scene-1', title: 'Opening', shots: [{
      id: 'shot-1', title: 'Hero', duration: 2, selected_take: 'take-1',
      takes: [{ id: 'take-1', panels: [{
        id: 'panel-1', time: 0, title: 'A better opening', direction: 'Hold on the entrance',
        layers: [{ type: 'image', src: 'sources/frames/hero.png' },
          { type: 'text', text: 'RED DOOR', label: 'Revision label' }],
      }] }],
    }] }],
  };
  const brief = revisionBrief('revision', doc, [], '/workspace', '/bin/storyboard');
  assert.match(brief, /Project path: \/workspace\/revision/);
  assert.match(brief, /panel-1 A better opening image sources: sources\/frames\/hero\.png/);
  assert.match(brief, /panel-1 direction: "Hold on the entrance"/);
  assert.match(brief, /panel-1 text layer "Revision label": "RED DOOR"/);
  assert.match(brief, /storyboard MCP show tool/);
  assert.doesNotMatch(brief, /Cuts:/);
});

test('cut placement voice-over is explicit in the Claude brief', () => {
  const doc = {
    title: 'Parallel scenes', fps: 24, format: { width: 1920, height: 1080 },
    scenes: [{ id: 'scene-a', title: 'A', shots: [{ id: 'shot-a', title: 'Return', duration: 2,
      source_audio: { src: 'sources/imported-audio/shot/original.wav', start: 0, end: 2 }, takes: [] }] }],
    cuts: [{ id: 'cut-a', name: 'Intercut', clips: [{ id: 'clip-a', source: { shot: { shot: 'shot-a' } },
      in: 12, source_audio: { src: 'sources/imported-audio/placement/original.wav', start: 0.5, end: 3.5 } }] }],
  };
  const brief = revisionBrief('parallel', doc);
  assert.match(brief, /Attached audio: sources\/imported-audio\/shot\/original\.wav, 0s–2s/);
  assert.match(brief, /Placement VO: sources\/imported-audio\/placement\/original\.wav, 0\.5s–3\.5s \(overrides shot VO for this placement\)/);
  assert.match(brief, /shot-a in=12f/);
});
