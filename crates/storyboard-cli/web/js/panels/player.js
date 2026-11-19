// First pass cut player: compiled shot order, storyboard panels, and source-movie audio/video.

import { api, commit, getProject, moveMovieBoundary, panelUrl, postJson, splitMovieShot, uploadAudio } from '../core/api.js';
import { h } from '../core/dom.js';
import { registerPanel } from '../core/registry.js';
import { notify, selectOnly, setPrimary, setShotUi, shotView, state } from '../core/store.js';
import { enc } from '../core/util.js';
import { clampMediaTime, shouldSyncMedia } from '../core/timing.js';

registerPanel({
  id: 'player', title: 'Player', region: 'toolbar', order: 33, dock: { defaultDock: 'bottom', shortcut: 'Mod+Alt+7' },
  mount(el, ctx) {
    const button = h('button', { class: 'tb-button', type: 'button', text: 'Play cut' });
    const picture = h('img', { alt: 'Storyboard panel', hidden: true });
    const video = h('video', { preload: 'metadata', playsinline: true, hidden: true });
    const audio = h('audio', { preload: 'metadata', hidden: true });
    const stage = h('div', { class: 'player-stage' }, picture, video, audio);
    const timeline = h('div', { class: 'player-timeline', 'aria-label': 'Cut timeline' });
    const position = h('input', { type: 'range', min: '0', max: '0', value: '0', step: '1', 'aria-label': 'Playhead frame' });
    const readout = h('span', { text: '0:00 / 0:00' });
    const info = h('div', { class: 'player-info' });
    const warning = h('div', { class: 'player-warning', role: 'status' });
    const play = h('button', { class: 'btn primary', type: 'button', text: 'Play' });
    const back = h('button', { class: 'btn', type: 'button', text: '−1 frame' });
    const forward = h('button', { class: 'btn', type: 'button', text: '+1 frame' });
    const split = h('button', { class: 'btn', type: 'button', text: 'Split placement', title: 'Split this cut placement at the current frame' });
    const merge = h('button', { class: 'btn', type: 'button', text: 'Merge placements', title: 'Merge adjacent parts of the same shot at the playhead' });
    const group = h('button', { class: 'btn', type: 'button', text: 'Group placements', title: 'Keep two revised shots and their media together as one movable cut placement' });
    const ungroup = h('button', { class: 'btn', type: 'button', text: 'Ungroup', title: 'Restore grouped placements to this cut' });
    const splitShots = h('button', { class: 'btn', type: 'button', text: 'Split into shots', title: 'Create two editable source-linked shots at the playhead' });
    const attachVoice = h('button', { class: 'btn', type: 'button', text: 'Attach placement VO', title: 'Attach audio to this cut placement only' });
    const voicePicker = h('input', { type: 'file', accept: '.mp3,.wav,.m4a,audio/mpeg,audio/wav,audio/mp4', hidden: true });
    const voiceStart = h('input', { type: 'number', min: '0', step: '0.01', 'aria-label': 'Placement VO start seconds' });
    const voiceEnd = h('input', { type: 'number', min: '0', step: '0.01', 'aria-label': 'Placement VO end seconds' });
    const saveVoice = h('button', { class: 'btn', type: 'button', text: 'Set VO range' });
    const removeVoice = h('button', { class: 'btn', type: 'button', text: 'Remove placement VO' });
    const voiceControls = h('div', { class: 'player-controls', hidden: true }, 'Placement VO from', voiceStart, 'to', voiceEnd, 'seconds', saveVoice, removeVoice);
    const mode = h('select', { 'aria-label': 'Playback image' },
      h('option', { value: 'boards', text: 'Storyboards + source audio' }),
      h('option', { value: 'original', text: 'Original movie + audio' }));
    const exportMovie = h('button', { class: 'btn', type: 'button', text: 'Export MP4', title: 'Render this cut using the selected image mode' });
    const close = h('button', { class: 'btn', type: 'button', text: 'Close' });
    const dialog = h('div', { class: 'player-dialog', role: 'dialog', 'aria-label': 'Cut player', hidden: true },
      stage, timeline, h('div', { class: 'player-controls' }, play, back, forward, split, merge, group, ungroup, splitShots, attachVoice, mode, exportMovie, close), voiceControls, position, readout, info, warning);
    el.append(button, dialog, voicePicker);
    let document = null;
    let sequence = null;
    let frame = 0;
    let playing = false;
    let tickId = 0;
    let started = 0;
    let startFrame = 0;
    let mediaKey = '';
    let audioKey = '';
    let imageKey = '';
    let selectionKey = '';
    let mediaProblem = '';
    let voiceEditKey = '';
    let loadEpoch = 0;
    let shownItem = null;
    let exactSeek = true;
    const waveformCache = new Map();
    const waveformQueue = [];
    let waveformActive = 0;
    const pumpWaveforms = () => {
      while (waveformActive < 2 && waveformQueue.length) {
        const { url, resolve } = waveformQueue.shift();
        waveformActive++;
        api(url).then((result) => resolve(result.peaks), () => resolve(null))
          .finally(() => { waveformActive--; pumpWaveforms(); });
      }
    };
    const addWaveform = (segment, source, kind, start, end, span, ext = '') => {
      if (end <= start) return;
      const folder = source.src.split('/')[2];
      const url = `/api/waveform?project=${enc(state.project)}&kind=${kind}&folder=${enc(folder)}&start=${start.toFixed(3)}&end=${end.toFixed(3)}${ext ? `&ext=${enc(ext)}` : ''}`;
      const canvas = h('canvas', { class: 'player-waveform', width: '128', height: '20', 'aria-label': `${kind === 'audio' ? 'Voice-over' : 'Source movie'} waveform` });
      segment.append(canvas);
      if (!waveformCache.has(url)) {
        waveformCache.set(url, new Promise((resolve) => { waveformQueue.push({ url, resolve }); pumpWaveforms(); }));
      }
      waveformCache.get(url).then((peaks) => {
        if (!peaks || !canvas.isConnected) return;
        const context = canvas.getContext('2d');
        const covered = Math.min(1, (end - start) * document.fps / span);
        context.fillStyle = '#86b7d6';
        peaks.forEach((peak, index) => {
          const height = Math.max(1, Math.round(Math.sqrt(peak / 32768) * 18));
          context.fillRect(index * covered, (20 - height) / 2, 1, height);
        });
      });
    };
    const clock = (seconds) => `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, '0')}`;
    const stop = () => { playing = false; cancelAnimationFrame(tickId); video.pause(); audio.pause(); play.textContent = 'Play'; };
    const leafClip = (item) => document?.cuts?.flatMap((candidate) => candidate.clips || [])
      .find((candidate) => candidate.id === (item?.leaf_clip || item?.clip));
    const mergePair = () => {
      const cut = document?.cuts?.find((candidate) => candidate.id === (state.viewCut || document.active_cut));
      const rightIndex = sequence?.items.findIndex((item) => item.start === frame);
      if (!cut || rightIndex == null || rightIndex < 1) return null;
      const leftItem = sequence.items[rightIndex - 1];
      const rightItem = sequence.items[rightIndex];
      if (leftItem.end !== frame || leftItem.nested || rightItem.nested) return null;
      const leftIndex = cut.clips.findIndex((clip) => clip.id === leftItem.clip);
      if (leftIndex < 0) return null;
      const left = cut.clips[leftIndex];
      const right = cut.clips[leftIndex + 1];
      if (!right || right.id !== rightItem.clip || !left.source.shot || !right.source.shot
          || left.slot != null || right.slot != null || left.out == null || left.out !== (right.in || 0)
          || JSON.stringify(left.source) !== JSON.stringify(right.source)
          || JSON.stringify(left.source_audio || null) !== JSON.stringify(right.source_audio || null)
          || Object.keys(right).some((key) => !['id', 'source', 'source_audio', 'in', 'out', 'slot'].includes(key))) return null;
      return { cut, left, right };
    };
    const groupPair = () => {
      const cut = document?.cuts?.find((candidate) => candidate.id === (state.viewCut || document.active_cut));
      const rightIndex = sequence?.items.findIndex((item) => item.start === frame);
      if (!cut || rightIndex == null || rightIndex < 1) return null;
      const leftItem = sequence.items[rightIndex - 1];
      const rightItem = sequence.items[rightIndex];
      if (leftItem.end !== frame || leftItem.nested || rightItem.nested) return null;
      const index = cut.clips.findIndex((clip) => clip.id === leftItem.clip);
      const left = cut.clips[index];
      const right = cut.clips[index + 1];
      if (!left?.source.shot || !right?.source.shot || right.id !== rightItem.clip
          || left.source.shot.shot === right.source.shot.shot) return null;
      return { cut, left, right };
    };
    const renderTimeline = () => {
      const shots = new Map((document?.scenes || []).flatMap((scene) => scene.shots.map((shot) => [shot.id, { shot, scene }])));
      const cut = document?.cuts?.find((candidate) => candidate.id === (state.viewCut || document.active_cut));
      const segments = [];
      const makeGap = (start, end) => {
        const gap = h('button', { class: 'player-segment player-gap', type: 'button', title: `Black padding: frames ${start}–${end}`, text: 'Black padding' });
        gap.style.flexGrow = String(end - start);
        gap.addEventListener('click', () => { stop(); seek(start); });
        segments.push(gap);
      };
      let cursor = 0;
      for (const [itemIndex, item] of (sequence?.items || []).entries()) {
        if (item.start > cursor) makeGap(cursor, item.start);
        const { shot, scene } = shots.get(item.shot) || {};
        const clip = leafClip(item);
        const nested = !!clip && !cut?.clips.some((candidate) => candidate.id === clip.id);
        const span = item.end - item.start;
        const movie = shot?.source_movie;
        const voice = clip?.source_audio || shot?.source_audio;
        const take = shot?.takes.find((candidate) => candidate.id === item.take) || shot?.takes[0];
        const label = `${scene?.title || scene?.id || 'Scene'} · ${shot?.title || item.shot}`;
        const content = [take?.panels.length ? `${take.panels.length} panels` : null, movie ? 'video' : null, voice ? 'VO' : null].filter(Boolean).join(' · ');
        const segment = h('div', { class: 'player-segment',
          title: `${label}: frames ${item.start}–${item.end}${content ? ` · ${content}` : ''}` },
          h('span', { class: 'player-segment-name', text: label }),
          h('span', { class: 'player-segment-media', text: `${content || 'still'} · ${(span / document.fps).toFixed(2)}s` }));
        segment.style.flexGrow = String(Math.max(1, span));
        for (const panel of take?.panels || []) {
          const boundary = Math.round(panel.time * document.fps) - (item.offset || 0);
          if (boundary <= 0 || boundary >= span) continue;
          const tick = h('span', { class: 'player-panel-boundary', title: `Panel ${panel.id} at ${panel.time}s` });
          tick.style.left = `${boundary / span * 100}%`;
          segment.append(tick);
        }
        const audioSource = voice || (movie && movie.audio !== false ? movie : null);
        if (audioSource) {
          const start = audioSource.start + (item.offset || 0) / document.fps;
          const end = Math.min(audioSource.end, start + span / document.fps);
          const ext = voice?.src.split('.').at(-1) || '';
          addWaveform(segment, audioSource, voice ? 'audio' : 'movie', start, end, span, ext);
        }
        const following = sequence.items[itemIndex + 1];
        const clipIndex = cut?.clips.findIndex((candidate) => candidate.id === clip?.id) ?? -1;
        const nextClip = clipIndex >= 0 ? cut.clips[clipIndex + 1] : null;
        const nextShot = shots.get(following?.shot)?.shot;
        const movieFolder = shot?.source_movie?.src.replace(/\/original\.mp4$/, '/');
        const editablePanel = (candidate) => candidate?.takes.length === 1
          && candidate.takes[0].panels.length === 1
          && candidate.takes[0].panels[0].time === 0
          && candidate.takes[0].panels[0].layers.length === 1
          && candidate.takes[0].panels[0].layers[0].src?.startsWith(movieFolder);
        const canRoll = clip?.source.shot && nextClip?.source.shot
          && !nested && !following?.nested && following.start === item.end
          && nextClip.id === following.clip && clip.in == null && clip.out == null && clip.slot == null
          && nextClip.in == null && nextClip.out == null && nextClip.slot == null
          && !clip.source.shot.take && !nextClip.source.shot.take
          && shot?.source_movie && nextShot?.source_movie && !shot.source_audio && !nextShot.source_audio
          && !clip.source_audio && !nextClip.source_audio
          && shot.source_movie.src === nextShot.source_movie.src
          && Math.abs(shot.source_movie.end - nextShot.source_movie.start) < 0.000001
          && Math.abs(shot.duration - (shot.source_movie.end - shot.source_movie.start)) <= 1 / document.fps
          && Math.abs(nextShot.duration - (nextShot.source_movie.end - nextShot.source_movie.start)) <= 1 / document.fps
          && editablePanel(shot) && editablePanel(nextShot)
          && (document.cuts || []).flatMap((candidate) => candidate.clips).filter((candidate) => candidate.source.shot?.shot === shot.id).length === 1
          && (document.cuts || []).flatMap((candidate) => candidate.clips).filter((candidate) => candidate.source.shot?.shot === nextShot.id).length === 1;
        const sourceLimit = (sourceShot, sourceClip) => sourceShot
          ? Math.max(Math.round(sourceShot.duration * document.fps),
            Math.round(Math.max(0, (sourceClip?.source_audio?.end || 0) - (sourceClip?.source_audio?.start || 0)) * document.fps))
          : 0;
        const leftLimit = sourceLimit(shot, clip);
        const rightLimit = sourceLimit(nextShot, nextClip);
        const leftIn = clip?.in || 0;
        const leftOut = Math.min(clip?.out ?? leftLimit, leftLimit);
        const rightIn = nextClip?.in || 0;
        const rightOut = Math.min(nextClip?.out ?? rightLimit, rightLimit);
        const minimumDelta = Math.max(leftIn + 1 - leftOut, -rightIn);
        const maximumDelta = Math.min(leftLimit - leftOut, rightOut - rightIn - 1);
        const canPlacementRoll = !canRoll && clip?.source.shot && nextClip?.source.shot
          && !nested && !following?.nested && following.start === item.end
          && nextClip.id === following.clip && clip.slot == null && nextClip.slot == null
          && shot && nextShot && minimumDelta <= maximumDelta
          && (minimumDelta < 0 || maximumDelta > 0);
        if (canRoll && following.end - item.start >= 2) {
          const minimum = item.start + 1;
          const maximum = following.end - 1;
          const original = item.end;
          const handle = h('span', { class: 'player-trim-handle player-boundary-handle', role: 'slider', tabindex: '0',
            'aria-label': `Source boundary after ${label}`, 'aria-valuemin': String(minimum),
            'aria-valuemax': String(maximum), 'aria-valuenow': String(original),
            title: 'Drag to move the source boundary between these two shots. Arrow keys adjust one frame.' });
          let drag = null;
          const valueAt = (clientX) => Math.max(minimum, Math.min(maximum,
            original + Math.round((clientX - drag.startX) * (following.end - item.start) / drag.width)));
          const preview = (value) => {
            handle.setAttribute('aria-valuenow', String(value));
            handle.title = `Source boundary at frame ${value}; both shots change length`;
          };
          const save = async (value, version = state.version) => {
            if (value === original) { show(); return; }
            try { await moveMovieBoundary(state.project, { cut: cut.id, left_clip: clip.id, frame: value }, version); }
            catch (error) { notify.error(error, 'Moving movie boundary'); preview(original); }
          };
          handle.addEventListener('pointerdown', (event) => {
            event.preventDefault(); event.stopPropagation(); stop();
            const nextSegment = segment.nextElementSibling;
            drag = { startX: event.clientX,
              width: Math.max(1, segment.getBoundingClientRect().width + (nextSegment?.getBoundingClientRect().width || 0)),
              project: state.project, version: state.version };
            handle.setPointerCapture(event.pointerId);
          });
          handle.addEventListener('pointermove', (event) => { if (drag) preview(valueAt(event.clientX)); });
          handle.addEventListener('pointerup', (event) => {
            if (!drag) return;
            const value = valueAt(event.clientX);
            const valid = state.project === drag.project && state.version === drag.version;
            const version = drag.version;
            drag = null; event.stopPropagation();
            if (valid) save(value, version);
          });
          handle.addEventListener('pointercancel', () => { drag = null; preview(original); show(); });
          handle.addEventListener('click', (event) => event.stopPropagation());
          handle.addEventListener('keydown', (event) => {
            if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
            event.preventDefault(); event.stopPropagation(); stop();
            const direction = event.key === 'ArrowRight' ? 1 : -1;
            const current = Number(handle.getAttribute('aria-valuenow'));
            const value = Math.max(minimum, Math.min(maximum, current + direction * (event.shiftKey ? 10 : 1)));
            preview(value); save(value);
          });
          segment.append(handle);
        } else if (canPlacementRoll) {
          const original = item.end;
          const minimum = original + minimumDelta;
          const maximum = original + maximumDelta;
          const handle = h('span', { class: 'player-trim-handle player-boundary-handle', role: 'slider', tabindex: '0',
            'aria-label': `Placement boundary after ${label}`, 'aria-valuemin': String(minimum),
            'aria-valuemax': String(maximum), 'aria-valuenow': String(original),
            title: 'Drag to roll the boundary between these placements without changing cut length. Arrow keys adjust one frame.' });
          let drag = null;
          const valueAt = (clientX) => Math.max(minimum, Math.min(maximum,
            original + Math.round((clientX - drag.startX) * (following.end - item.start) / drag.width)));
          const preview = (value) => {
            handle.setAttribute('aria-valuenow', String(value));
            handle.title = `Placement boundary at frame ${value}; cut length stays fixed`;
          };
          const save = async (value) => {
            if (value === original) { show(); return; }
            const delta = value - original;
            const result = await commit([
              { update_clip: { cut: cut.id, clip: clip.id, patch: { out: leftOut + delta } } },
              { update_clip: { cut: cut.id, clip: nextClip.id, patch: { in: rightIn + delta } } },
            ], { what: 'Roll placement boundary' });
            if (!result) preview(original);
          };
          handle.addEventListener('pointerdown', (event) => {
            event.preventDefault(); event.stopPropagation(); stop();
            const nextSegment = segment.nextElementSibling;
            drag = { startX: event.clientX,
              width: Math.max(1, segment.getBoundingClientRect().width + (nextSegment?.getBoundingClientRect().width || 0)),
              project: state.project, version: state.version };
            handle.setPointerCapture(event.pointerId);
          });
          handle.addEventListener('pointermove', (event) => { if (drag) preview(valueAt(event.clientX)); });
          handle.addEventListener('pointerup', (event) => {
            if (!drag) return;
            const value = valueAt(event.clientX);
            const valid = state.project === drag.project && state.version === drag.version;
            drag = null; event.stopPropagation();
            if (valid) save(value);
          });
          handle.addEventListener('pointercancel', () => { drag = null; preview(original); show(); });
          handle.addEventListener('click', (event) => event.stopPropagation());
          handle.addEventListener('keydown', (event) => {
            if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
            event.preventDefault(); event.stopPropagation(); stop();
            const direction = event.key === 'ArrowRight' ? 1 : -1;
            const current = Number(handle.getAttribute('aria-valuenow'));
            const value = Math.max(minimum, Math.min(maximum, current + direction * (event.shiftKey ? 10 : 1)));
            preview(value); save(value);
          });
          segment.append(handle);
        } else if (clip?.source.shot && clip.slot == null && shot && !nested) {
          const limit = sourceLimit(shot, clip);
          const input = clip.in || 0;
          const minimum = input + 1;
          const original = clip.out ?? limit;
          if (limit > minimum) {
            const handle = h('span', { class: 'player-trim-handle', role: 'slider', tabindex: '0',
              'aria-label': `End frame for ${label}`, 'aria-valuemin': String(minimum),
              'aria-valuemax': String(limit), 'aria-valuenow': String(original),
              title: 'Drag to trim this placement; later clips ripple. Arrow keys adjust one frame.' });
            let drag = null;
            const valueAt = (clientX) => Math.max(minimum, Math.min(limit,
              original + Math.round((clientX - drag.startX) * span / drag.width)));
            const preview = (value) => {
              handle.setAttribute('aria-valuenow', String(value));
              handle.title = `End at frame ${value} · ${(value - input) / document.fps}s; later clips ripple`;
            };
            const save = (value) => {
              if (value === original) { show(); return; }
              commit([{ update_clip: { cut: cut.id, clip: clip.id, patch: { out: value } } }], { what: 'Trim placement end' });
            };
            handle.addEventListener('pointerdown', (event) => {
              event.preventDefault(); event.stopPropagation(); stop();
              drag = { startX: event.clientX, width: Math.max(1, segment.getBoundingClientRect().width),
                project: state.project, version: state.version };
              handle.setPointerCapture(event.pointerId);
            });
            handle.addEventListener('pointermove', (event) => {
              if (drag) preview(valueAt(event.clientX));
            });
            handle.addEventListener('pointerup', (event) => {
              if (!drag) return;
              const value = valueAt(event.clientX);
              const valid = state.project === drag.project && state.version === drag.version;
              drag = null;
              event.stopPropagation();
              if (valid) save(value);
            });
            handle.addEventListener('pointercancel', () => { drag = null; preview(original); show(); });
            handle.addEventListener('click', (event) => event.stopPropagation());
            handle.addEventListener('keydown', (event) => {
              if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
              event.preventDefault(); event.stopPropagation(); stop();
              const direction = event.key === 'ArrowRight' ? 1 : -1;
              const current = Number(handle.getAttribute('aria-valuenow'));
              const value = Math.max(minimum, Math.min(limit, current + direction * (event.shiftKey ? 10 : 1)));
              preview(value); save(value);
            });
            segment.append(handle);
          }
        }
        segment.addEventListener('click', () => { stop(); seek(item.start); });
        segments.push(segment);
        cursor = Math.max(cursor, item.end);
      }
      if (cursor < (sequence?.frames || 0)) makeGap(cursor, sequence.frames);
      timeline.replaceChildren(...segments);
    };
    const show = () => {
      if (!sequence || !document) return;
      const fps = document.fps;
      const total = sequence.frames || 0;
      frame = Math.max(0, Math.min(frame, Math.max(0, total - 1)));
      position.max = String(Math.max(0, total - 1));
      position.value = String(frame);
      readout.textContent = `${clock(frame / fps)} / ${clock(total / fps)}  (${frame + 1}/${total} f)`;
      const item = sequence.items?.find((entry) => frame >= entry.start && frame < entry.end);
      const syncExactly = exactSeek || item !== shownItem;
      shownItem = item;
      exactSeek = false;
      const cut = document.cuts?.find((candidate) => candidate.id === (state.viewCut || document.active_cut));
      const placed = leafClip(item);
      const nested = !!placed && !cut?.clips.some((candidate) => candidate.id === placed.id);
      attachVoice.disabled = !placed?.source.shot || nested;
      attachVoice.title = attachVoice.disabled ? 'Select a shot placement in the cut' : 'Attach audio to this placement only';
      voiceControls.hidden = nested || !placed?.source_audio;
      const nextVoiceEditKey = placed?.source_audio ? `${placed.id}/${JSON.stringify(placed.source_audio)}` : '';
      if (nextVoiceEditKey !== voiceEditKey) {
        voiceEditKey = nextVoiceEditKey;
        voiceStart.value = placed?.source_audio?.start ?? '';
        voiceEnd.value = placed?.source_audio?.end ?? '';
      }
      split.disabled = !placed || nested || !placed.source.shot || placed.slot != null || frame <= item.start || frame >= item.end - 1;
      merge.disabled = !mergePair();
      group.disabled = !groupPair();
      const wrapper = cut?.clips.find((candidate) => candidate.id === item?.clip && candidate.source.cut);
      ungroup.disabled = !wrapper;
      split.title = placed?.slot != null ? 'Remove the fixed slot before splitting this placement' : 'Split this cut placement at the current frame';
      const shot = document.scenes.flatMap((scene) => scene.shots).find((candidate) => candidate.id === item?.shot);
      const scene = document.scenes.find((candidate) => candidate.shots.some((entry) => entry.id === shot?.id));
      const canFork = !!placed?.source.shot && !nested && placed.slot == null
        && placed.source.shot.take == null && frame > item.start && frame < item.end - 1;
      splitShots.disabled = !canFork;
      splitShots.title = canFork
        ? 'Split this placement into two independently editable shots'
        : 'Select an interior frame of an individual shot without a fixed slot or take override';
      if (!shot || !item) { picture.hidden = true; video.hidden = true; video.pause(); audio.pause(); info.textContent = 'No shot at this frame.'; warning.textContent = ''; return; }
      const local = (frame - item.start + (item.offset || 0)) / fps;
      const take = shot.takes.find((candidate) => candidate.id === item.take) || shot.takes[0];
      const panel = take?.panels.filter((candidate) => candidate.time <= local).at(-1) || take?.panels[0];
      const key = panel && `${shot.id}/${take.id}/${panel.id}`;
      const nextSelection = `${shot.id}/${take?.id || ''}/${panel?.id || ''}`;
      if (nextSelection !== selectionKey) {
        selectionKey = nextSelection;
        shotView(shot);
        setShotUi(shot.id, { take: take?.id || null, panel: panel?.id || null });
        if (state.primary?.type === 'shot' && state.primary.id === shot.id && state.sel.shots.has(shot.id)) {
          setPrimary('shot', shot.id);
        } else {
          selectOnly('shot', shot.id);
        }
      }
      if (key && key !== imageKey) { picture.src = panelUrl(shot.id, take.id, panel, 1280); imageKey = key; }
      const movie = shot.source_movie;
      const voice = placed?.source_audio || shot.source_audio;
      const nextMedia = movie ? `${state.project}/${movie.src}` : '';
      if (nextMedia !== mediaKey) {
        video.pause(); mediaKey = nextMedia; mediaProblem = '';
        if (movie) {
          const folder = movie.src.split('/')[2];
          video.src = `/api/movie.mp4?project=${enc(state.project)}&folder=${enc(folder)}`;
          video.load();
        } else { video.removeAttribute('src'); video.load(); }
      }
      const sourceTime = movie ? clampMediaTime(movie.start, movie.end, movie.start + local) : 0;
      const hold = !!movie && movie.start + local >= movie.end;
      video.muted = !!voice;
      if (movie && shouldSyncMedia(video.currentTime, sourceTime, fps, syncExactly)) {
        video.currentTime = sourceTime;
      }
      if (movie && playing && !hold && video.paused) video.play().catch(() => stop());
      if ((!movie || hold) && !video.paused) video.pause();
      const nextAudio = voice ? `${state.project}/${voice.src}` : '';
      if (nextAudio !== audioKey) {
        audio.pause(); audioKey = nextAudio; mediaProblem = '';
        if (voice) {
          const parts = voice.src.split('/');
          audio.src = `/api/audio?project=${enc(state.project)}&folder=${enc(parts[2])}&ext=${enc(parts[3].split('.').at(-1))}`;
          audio.load();
        } else { audio.removeAttribute('src'); audio.load(); }
      }
      const voiceTime = voice ? clampMediaTime(voice.start, voice.end, voice.start + local) : 0;
      const voiceDone = !!voice && voice.start + local >= voice.end;
      if (voice && shouldSyncMedia(audio.currentTime, voiceTime, fps, syncExactly)) audio.currentTime = voiceTime;
      if (voice && playing && !voiceDone && audio.paused) audio.play().catch(() => stop());
      if ((!voice || voiceDone) && !audio.paused) audio.pause();
      picture.hidden = !panel || (mode.value === 'original' && !!movie);
      video.hidden = mode.value !== 'original' || !movie;
      info.textContent = `${scene?.title || scene?.id || 'Scene'} · ${shot.title || shot.id} · ${Math.round(local * 100) / 100}s in shot`;
      const movieSpan = movie ? movie.end - movie.start : 0;
      const voiceSpan = voice ? voice.end - voice.start : 0;
      const placementEnd = ((item.offset || 0) + item.end - item.start) / fps;
      const unmatchedVoice = Math.max(0, Math.min(voiceSpan, placementEnd) - movieSpan);
      const timingWarning = movie && unmatchedVoice > 0
        ? `Voice-over is ${Math.round(unmatchedVoice * 100) / 100}s longer than the video in this placement. Holding the last frame after ${Math.round(movieSpan * 100) / 100}s.`
        : hold ? `Source video ends ${Math.round((local - movieSpan) * 100) / 100}s before this placement. Holding the last frame.` : '';
      warning.textContent = [timingWarning, mediaProblem].filter(Boolean).join(' ');
    };
    const tick = (now) => {
      if (!playing) return;
      frame = Math.min((sequence.frames || 1) - 1, startFrame + Math.floor((now - started) * document.fps / 1000));
      show();
      if (frame >= sequence.frames - 1) { stop(); return; }
      tickId = requestAnimationFrame(tick);
    };
    const toggle = () => {
      if (playing) { stop(); return; }
      if (!sequence?.frames) return;
      if (frame >= sequence.frames - 1) frame = 0;
      playing = true; play.textContent = 'Pause'; started = performance.now(); startFrame = frame;
      show(); tickId = requestAnimationFrame(tick);
    };
    const seek = (value) => { frame = Number(value); exactSeek = true; if (playing) { started = performance.now(); startFrame = frame; } show(); };
    const reload = async (reset) => {
      if (!state.project) return;
      const epoch = ++loadEpoch;
      const project = state.project;
      button.disabled = true;
      try {
        const result = await getProject(project, state.viewCut);
        if (epoch !== loadEpoch || state.project !== project) return;
        stop();
        document = result.document; sequence = result.sequence; selectionKey = '';
        shownItem = null; exactSeek = true;
        if (reset) { frame = 0; mediaKey = ''; audioKey = ''; imageKey = ''; }
        dialog.hidden = false; renderTimeline(); show();
      } catch (error) { info.textContent = error.message; dialog.hidden = false; }
      finally { button.disabled = false; }
    };
    button.addEventListener('click', () => reload(true));
    attachVoice.addEventListener('click', () => voicePicker.click());
    saveVoice.addEventListener('click', async () => {
      const cut = document?.cuts?.find((candidate) => candidate.id === (state.viewCut || document.active_cut));
      const item = sequence?.items.find((entry) => frame >= entry.start && frame < entry.end);
      const clip = cut?.clips.find((candidate) => candidate.id === item?.clip);
      if (!clip?.source_audio || !state.project) return;
      const start = Number(voiceStart.value);
      const end = Number(voiceEnd.value);
      const limit = audio.duration;
      const roundedStart = Math.round(start * document.fps) / document.fps;
      const roundedEnd = Math.min(limit, Math.round(end * document.fps) / document.fps);
      if (!Number.isFinite(start) || !Number.isFinite(end) || !Number.isFinite(limit)
          || start < 0 || roundedEnd <= roundedStart || end > limit + 1 / document.fps) {
        notify.status({ text: 'Set a VO range within the loaded audio file.', error: true });
        return;
      }
      saveVoice.disabled = true;
      try {
        await commit([{ update_clip: { cut: cut.id, clip: clip.id, patch: { source_audio: {
          ...clip.source_audio,
          start: roundedStart,
          end: roundedEnd,
        } } } }], { what: 'Set placement VO range' });
      } catch (error) { notify.error(error, 'Setting placement VO range'); }
      finally { saveVoice.disabled = false; }
    });
    removeVoice.addEventListener('click', async () => {
      const cut = document?.cuts?.find((candidate) => candidate.id === (state.viewCut || document.active_cut));
      const item = sequence?.items.find((entry) => frame >= entry.start && frame < entry.end);
      const clip = cut?.clips.find((candidate) => candidate.id === item?.clip);
      if (!clip?.source_audio) return;
      removeVoice.disabled = true;
      try { await commit([{ update_clip: { cut: cut.id, clip: clip.id, patch: { source_audio: null } } }], { what: 'Remove placement VO' }); }
      catch (error) { notify.error(error, 'Removing placement VO'); }
      finally { removeVoice.disabled = false; }
    });
    voicePicker.addEventListener('change', async () => {
      const file = voicePicker.files?.[0];
      const cut = document?.cuts?.find((candidate) => candidate.id === (state.viewCut || document.active_cut));
      const item = sequence?.items.find((entry) => frame >= entry.start && frame < entry.end);
      const clip = cut?.clips.find((candidate) => candidate.id === item?.clip);
      const project = state.project;
      if (!file || !project || !clip?.source.shot || item.nested) return;
      const ext = file.name.split('.').at(-1)?.toLowerCase();
      if (!['mp3', 'wav', 'm4a'].includes(ext) || !file.size || file.size > 128 * 1024 * 1024) {
        notify.status({ text: 'Choose an MP3, WAV or M4A under 128 MB.', error: true });
        voicePicker.value = '';
        return;
      }
      attachVoice.disabled = true;
      try {
        const uploaded = await uploadAudio(project, crypto.randomUUID(), ext, file);
        if (state.project !== project) throw new Error('Project changed during audio upload.');
        const current = state.doc?.cuts?.find((candidate) => candidate.id === cut.id)?.clips.find((candidate) => candidate.id === clip.id);
        if (!current) throw new Error('Placement changed during audio upload.');
        await commit([{ update_clip: { cut: cut.id, clip: clip.id, patch: {
          source_audio: { src: uploaded.path, start: 0, end: uploaded.media.duration },
        } } }], { what: 'Attach placement voice-over' });
      } catch (error) { notify.error(error, 'Attaching placement voice-over'); }
      finally { voicePicker.value = ''; show(); }
    });
    play.addEventListener('click', toggle);
    back.addEventListener('click', () => { stop(); seek(frame - 1); });
    forward.addEventListener('click', () => { stop(); seek(frame + 1); });
    split.addEventListener('click', async () => {
      const cut = document?.cuts?.find((candidate) => candidate.id === (state.viewCut || document.active_cut));
      const item = sequence?.items.find((entry) => frame > entry.start && frame < entry.end - 1);
      const index = cut?.clips.findIndex((candidate) => candidate.id === item?.clip) ?? -1;
      const clip = index >= 0 ? cut.clips[index] : null;
      if (!clip?.source.shot || clip.slot != null) return;
      const boundary = (item.offset || 0) + frame - item.start;
      if (boundary <= (clip.in || 0) || (clip.out != null && boundary >= clip.out)) return;
      stop();
      split.disabled = true;
      const newClip = `clip-${crypto.randomUUID()}`;
      await commit([
        { update_clip: { cut: cut.id, clip: clip.id, patch: { out: boundary } } },
        { add_clip: { cut: cut.id, id: newClip, source: clip.source, index: index + 1 } },
        { update_clip: { cut: cut.id, clip: newClip, patch: { in: boundary, out: clip.out ?? null, source_audio: clip.source_audio || null } } },
      ], { what: 'Split placement' });
    });
    merge.addEventListener('click', async () => {
      const pair = mergePair();
      if (!pair) return;
      stop(); merge.disabled = true;
      const result = await commit([{ merge_clips: { cut: pair.cut.id, left: pair.left.id, right: pair.right.id } }], { what: 'Merge placements' });
      if (!result) show();
    });
    group.addEventListener('click', async () => {
      const pair = groupPair();
      if (!pair) return;
      stop(); group.disabled = true;
      const result = await commit([{ group_clips: { cut: pair.cut.id, left: pair.left.id, right: pair.right.id,
        name: `${pair.left.id} + ${pair.right.id}` } }], { what: 'Group placements' });
      if (!result) show();
    });
    ungroup.addEventListener('click', async () => {
      const cut = document?.cuts?.find((candidate) => candidate.id === (state.viewCut || document.active_cut));
      const item = sequence?.items.find((entry) => frame >= entry.start && frame < entry.end);
      const wrapper = cut?.clips.find((candidate) => candidate.id === item?.clip && candidate.source.cut);
      if (!wrapper) return;
      stop(); ungroup.disabled = true;
      const result = await commit([{ ungroup_clips: { cut: cut.id, clip: wrapper.id } }], { what: 'Ungroup placements' });
      if (!result) show();
    });
    splitShots.addEventListener('click', async () => {
      const cut = document?.cuts?.find((candidate) => candidate.id === (state.viewCut || document.active_cut));
      const item = sequence?.items.find((entry) => frame > entry.start && frame < entry.end - 1);
      const index = cut?.clips.findIndex((candidate) => candidate.id === item?.clip) ?? -1;
      const clip = index >= 0 ? cut.clips[index] : null;
      const shot = document?.scenes.flatMap((scene) => scene.shots).find((candidate) => candidate.id === clip?.source.shot?.shot);
      if (!clip?.source.shot || !shot || !state.project || item.nested || clip.slot != null || clip.source.shot.take != null) return;
      const boundary = (item.offset || 0) + frame - item.start;
      if (boundary <= (clip.in || 0) || (clip.out != null && boundary >= clip.out)) return;
      const movie = shot.source_movie;
      const panel = shot.takes.length === 1 && shot.takes[0].panels.length === 1 ? shot.takes[0].panels[0] : null;
      const sourceFolder = movie?.src.match(/^sources\/imported-movies\/([0-9a-f-]{36})\/original\.mp4$/)?.[1];
      const sourceSplit = !!sourceFolder && !shot.source_audio && !clip.source_audio && (clip.in || 0) === 0 && clip.out == null
        && (item.offset || 0) === 0 && Math.abs(shot.duration - (movie.end - movie.start)) <= 1 / document.fps
        && panel?.time === 0 && panel.layers.length === 1 && panel.layers[0].type === 'image'
        && panel.layers[0].src?.startsWith(`sources/imported-movies/${sourceFolder}/`)
        && document.cuts.flatMap((candidate) => candidate.clips)
          .filter((candidate) => candidate.source.shot?.shot === shot.id).length === 1;
      stop(); splitShots.disabled = true;
      try {
        if (sourceSplit) {
          await splitMovieShot(state.project, { cut: cut.id, clip: item.clip, frame });
        } else {
          const newShot = `shot-${crypto.randomUUID()}`;
          const newClip = `clip-${crypto.randomUUID()}`;
          const newSource = structuredClone(clip.source);
          newSource.shot.shot = newShot;
          const result = await commit([
            { duplicate_shot: { shot: shot.id, id: newShot } },
            { update_clip: { cut: cut.id, clip: clip.id, patch: { out: boundary } } },
            { add_clip: { cut: cut.id, id: newClip, source: newSource, index: index + 1 } },
            { update_clip: { cut: cut.id, clip: newClip, patch: { in: boundary, out: clip.out ?? null, source_audio: clip.source_audio || null } } },
          ], { what: 'Split into shots' });
          if (!result) show();
        }
      } catch (error) { notify.error(error, 'Splitting shot'); show(); }
    });
    exportMovie.addEventListener('click', async () => {
      if (!state.project || !document || !sequence?.frames) return;
      const project = state.project;
      const cut = state.viewCut || document.active_cut;
      exportMovie.disabled = true;
      exportMovie.textContent = 'Rendering MP4…';
      warning.textContent = 'Rendering the cut. This may take a few minutes.';
      try {
        const result = await postJson('/api/export-movie', {
          project, base_version: state.version, cut, mode: mode.value,
        });
        if (state.project !== project) return;
        const link = h('a', { href: `/api/export.mp4?project=${enc(project)}&file=${enc(result.file)}`, download: result.file });
        globalThis.document.body.append(link);
        link.click();
        link.remove();
        notify.status({ text: `Rendered ${result.file}`, at: new Date().toISOString() });
        show();
      } catch (error) {
        if (state.project === project) notify.error(error, 'Exporting MP4');
        show();
      } finally {
        exportMovie.disabled = false;
        exportMovie.textContent = 'Export MP4';
      }
    });
    position.addEventListener('input', () => seek(position.value));
    mode.addEventListener('change', show);
    video.addEventListener('error', () => { mediaProblem = 'Could not load this movie. Try the storyboard view or replace the clip.'; show(); });
    audio.addEventListener('error', () => { mediaProblem = 'Could not load this audio. Check the attached source or replace it.'; show(); });
    const dismiss = () => { stop(); dialog.hidden = true; video.removeAttribute('src'); video.load(); audio.removeAttribute('src'); audio.load(); mediaKey = ''; audioKey = ''; shownItem = null; exactSeek = true; };
    close.addEventListener('click', dismiss);
    ctx.store.on('beforeProjectChange', () => { loadEpoch++; stop(); dialog.hidden = true; selectionKey = ''; });
    ctx.store.on('doc', () => { button.hidden = !state.doc; if (!state.doc) { loadEpoch++; stop(); dialog.hidden = true; } else if (!dialog.hidden) reload(false); });
    button.hidden = !state.doc;
    return { visibilityChanged(open) { if (!open) dismiss(); } };
  },
});
