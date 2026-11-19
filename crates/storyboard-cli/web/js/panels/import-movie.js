// Bring an MP4 rough cut into the project as editable, source-linked shots.

import { importMovie, postJson, uploadMovie } from '../core/api.js';
import { h } from '../core/dom.js';
import { registerPanel } from '../core/registry.js';
import { notify, state } from '../core/store.js';
import { enc } from '../core/util.js';

registerPanel({
  id: 'import-movie', title: 'Import movie', region: 'toolbar', order: 32,
  mount(el, ctx) {
    const button = h('button', { class: 'tb-button', type: 'button', text: 'Import MP4' });
    const picker = h('input', { type: 'file', accept: '.mp4,video/mp4', hidden: true });
    const form = h('form', { class: 'import-shot-form import-movie-form', hidden: true });
    const info = h('div', { class: 'import-shot-muted', role: 'status' });
    const scene = h('select', { 'aria-label': 'Scene' });
    const title = h('input', { type: 'text', required: true, maxlength: 200, 'aria-label': 'Movie title' });
    const interval = h('input', { type: 'number', required: true, min: '0.25', max: '60', step: 'any', value: '5', 'aria-label': 'Seconds per shot' });
    const cutTimes = h('input', { type: 'text', placeholder: 'e.g. 12.5, 28, 46.25', 'aria-label': 'Cut at times in seconds' });
    const preview = h('video', { class: 'import-movie-preview', controls: true, preload: 'metadata', 'aria-label': 'Movie preview' });
    const addCut = h('button', { class: 'btn', type: 'button', text: 'Add cut at playhead', disabled: true });
    const threshold = h('input', { type: 'number', min: '0.1', max: '50', step: 'any', value: '0.5', 'aria-label': 'Scene change threshold percent' });
    const suggest = h('button', { class: 'btn', type: 'button', text: 'Suggest cuts', disabled: true });
    const marksList = h('div', { class: 'import-movie-marks', 'aria-label': 'Cut marks' });
    const error = h('div', { class: 'import-shot-error', role: 'alert', hidden: true });
    const submit = h('button', { class: 'btn primary', type: 'submit', text: 'Create boards' });
    const cancel = h('button', { class: 'btn', type: 'button', text: 'Cancel' });
    form.append(h('strong', { text: 'Import MP4 as boards' }), info, preview, addCut,
      h('div', { class: 'import-movie-suggest' }, h('label', null, 'Scene change threshold (%)', threshold), suggest),
      h('label', null, 'Scene', scene),
      h('label', null, 'Title', title),
      h('label', null, 'Seconds per shot', interval),
      h('label', null, 'Cut at times in seconds (optional)', cutTimes),
      marksList,
      h('small', { class: 'import-shot-muted', text: 'Suggested cuts are visual changes, not guaranteed scene boundaries. Review marks before creating boards. Leave marks blank to sample at the interval above.' }), error,
      h('div', { class: 'import-shot-actions' }, cancel, submit));
    el.append(button, picker, form);
    let upload = null;
    let busy = false;
    const parseMarks = () => cutTimes.value.trim() ? cutTimes.value.trim().split(/[\s,]+/).map(Number) : [];
    const clearPreview = () => { preview.pause(); preview.removeAttribute('src'); preview.load(); addCut.disabled = true; };
    const showError = (message) => { error.textContent = message; error.hidden = false; };
    const renderMarks = () => {
      marksList.replaceChildren(...parseMarks().filter(Number.isFinite).map((mark) => {
        const seek = h('button', { type: 'button', class: 'import-movie-seek',
          'aria-label': `Seek preview to cut at ${mark} seconds`, text: `${mark} s` });
        seek.addEventListener('click', () => {
          if (!upload || mark < 0 || mark >= upload.media.duration) return;
          preview.pause();
          preview.currentTime = mark;
        });
        const remove = h('button', { type: 'button', class: 'import-movie-remove', 'aria-label': `Remove cut at ${mark} seconds`, text: '×' });
        remove.addEventListener('click', () => {
          cutTimes.value = parseMarks().filter((item) => item !== mark).join(', ');
          cutTimes.dispatchEvent(new Event('input'));
        });
        return h('span', { class: 'import-movie-mark' }, seek, remove);
      }));
    };
    const sync = () => {
      button.hidden = !state.doc; button.disabled = !state.doc || busy; suggest.disabled = !upload || busy;
      if (!state.doc && !busy) { form.hidden = true; upload = null; clearPreview(); }
    };
    button.addEventListener('click', () => picker.click());
    cutTimes.addEventListener('input', () => { interval.disabled = !!cutTimes.value.trim(); renderMarks(); });
    preview.addEventListener('loadedmetadata', () => { addCut.disabled = !upload; });
    preview.addEventListener('error', () => { addCut.disabled = true; if (upload) showError('Movie preview could not load. You can still enter cut times.'); });
    addCut.addEventListener('click', () => {
      if (!upload || !state.doc || !Number.isFinite(preview.currentTime)) return;
      const fps = state.doc.fps;
      const frame = Math.round(preview.currentTime * fps);
      const mark = frame / fps;
      const marks = parseMarks();
      if (marks.some((item) => !Number.isFinite(item))) return showError('Fix the cut times before adding another mark.');
      if (frame < 1 || upload.media.duration - mark < 1 / fps - 1e-7) return showError('Place the playhead at least one project frame from either end.');
      if (marks.some((item) => Math.round(item * fps) === frame)) return showError('There is already a cut at this frame.');
      if (marks.length >= 199) return showError('A movie can have at most 199 cut marks.');
      marks.push(mark);
      marks.sort((a, b) => a - b);
      cutTimes.value = marks.map((item) => Number(item.toFixed(6))).join(', ');
      cutTimes.dispatchEvent(new Event('input'));
      error.hidden = true;
    });
    suggest.addEventListener('click', async () => {
      if (!upload || busy || !state.doc) return;
      const percent = Number(threshold.value);
      if (!Number.isFinite(percent) || percent < 0.1 || percent > 50) return showError('Use a scene change threshold from 0.1 to 50%.');
      const current = parseMarks();
      if (current.some((mark) => !Number.isFinite(mark))) return showError('Fix the cut times before adding suggestions.');
      const project = upload.project;
      busy = true; sync(); submit.disabled = true; cancel.disabled = true; error.hidden = true;
      info.textContent = 'Scanning the copied MP4 for visual cuts…';
      try {
        const result = await postJson('/api/suggest-cuts', { project, folder: upload.folder, threshold: percent });
        if (state.project !== project) throw new Error('Project changed during cut scan.');
        const fps = state.doc.fps;
        const frames = new Set([...current, ...result.cut_times].map((mark) => Math.round(mark * fps)));
        if (frames.size >= 200) throw new Error('Over 199 marks after combining suggestions. Remove some manual marks or raise the threshold.');
        cutTimes.value = [...frames].sort((a, b) => a - b).map((frame) => Number((frame / fps).toFixed(6))).join(', ');
        cutTimes.dispatchEvent(new Event('input'));
        info.textContent = result.cut_times.length
          ? `Found ${result.cut_times.length} visual cut suggestion${result.cut_times.length === 1 ? '' : 's'}. Review the marks and preview before creating boards.`
          : 'No visual cuts found at this threshold. Add marks manually or use interval sampling.';
      } catch (failure) { showError(failure.message); notify.error(failure, 'Suggesting movie cuts'); }
      finally { busy = false; submit.disabled = false; cancel.disabled = false; sync(); }
    });
    cancel.addEventListener('click', () => { if (!busy) { form.hidden = true; picker.value = ''; upload = null; clearPreview(); } });
    picker.addEventListener('change', async () => {
      const file = picker.files?.[0];
      if (!file || !state.project) return;
      error.hidden = true;
      if (!/\.mp4$/i.test(file.name) || !file.size || file.size > 1024 * 1024 * 1024) {
        error.textContent = 'Choose a nonempty MP4 through 1 GB.'; error.hidden = false; form.hidden = false; submit.disabled = true; return;
      }
      const project = state.project;
      const folder = crypto.randomUUID();
      upload = null; clearPreview(); marksList.replaceChildren();
      busy = true; sync();
      form.hidden = false; submit.disabled = true; cancel.disabled = true;
      info.textContent = `Copying ${file.name} into project sources…`;
      try {
        upload = { ...(await uploadMovie(project, folder, file)), folder, project };
        if (state.project !== project) throw new Error('Project changed during upload.');
        const media = upload.media;
        info.textContent = `${file.name}: ${media.duration.toFixed(2)} s, ${media.width}×${media.height}, ${media.codec}, ${media.frame_rate} fps, ${media.audio ? 'audio' : 'silent'}. Original copied to project sources.`;
        scene.replaceChildren(h('option', { value: '', text: 'New scene' }),
          ...(state.doc.scenes || []).map((item) => h('option', { value: item.id, text: item.title || item.id })));
        title.value = file.name.replace(/\.mp4$/i, '');
        cutTimes.value = ''; cutTimes.dispatchEvent(new Event('input'));
        preview.src = `/api/movie.mp4?project=${enc(project)}&folder=${enc(folder)}`;
        preview.load();
        submit.disabled = false; suggest.disabled = false;
      } catch (failure) {
        upload = null; clearPreview();
        error.textContent = failure.message; error.hidden = false;
        info.textContent = 'Movie import failed.';
        notify.error(failure, 'Importing movie');
      } finally { busy = false; cancel.disabled = false; sync(); }
    });
    form.addEventListener('submit', async (event) => {
      event.preventDefault();
      if (!upload || busy || state.project !== upload.project) return;
      const seconds = Number(interval.value);
      const marks = parseMarks();
      if (marks.some((mark) => !Number.isFinite(mark)) || marks.length >= 200) {
        error.textContent = 'Enter up to 199 comma-separated cut times in seconds.'; error.hidden = false; return;
      }
      if (!marks.length && (!Number.isFinite(seconds) || seconds < 0.25 || seconds > 60)) {
        error.textContent = 'Use 0.25 to 60 seconds per shot.'; error.hidden = false; return;
      }
      busy = true; submit.disabled = true; cancel.disabled = true; sync(); error.hidden = true;
      info.textContent = 'Extracting frames and creating boards…';
      try {
        const beforeShots = (state.doc.scenes || []).reduce((count, item) => count + (item.shots || []).length, 0);
        const result = await importMovie(upload.project, { folder: upload.folder, scene: scene.value || null, title: title.value.trim(), interval: marks.length ? 5 : seconds, cut_times: marks });
        if (result) {
          const afterShots = (result.document.scenes || []).reduce((count, item) => count + (item.shots || []).length, 0);
          info.textContent = `Created ${afterShots - beforeShots} editable shots.`;
          form.hidden = true; picker.value = ''; upload = null; clearPreview();
        }
      } catch (failure) {
        error.textContent = failure.message; error.hidden = false;
        notify.error(failure, 'Creating movie boards');
      } finally { busy = false; submit.disabled = false; cancel.disabled = false; sync(); }
    });
    for (const event of ['doc', 'loading', 'projectClosed']) ctx.store.on(event, sync);
    sync();
    return {};
  },
});
