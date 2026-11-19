// Import numbered images as one shot without blocking the graph while files upload.

import { importShot, uploadShotImage } from '../core/api.js';
import { h } from '../core/dom.js';
import { imageSelectionError } from '../core/import-shot.js';
import { registerPanel } from '../core/registry.js';
import { notify, selectOnly, state } from '../core/store.js';

function selectedShot(sceneId) {
  const primary = state.primary;
  if (primary?.type !== 'shot') return null;
  const entry = state.model.shots.get(primary.id);
  return entry?.scene.id === sceneId ? primary.id : null;
}

registerPanel({
  id: 'import-shot', title: 'Import images', region: 'toolbar', order: 31,
  mount(el, ctx) {
    const button = h('button', { class: 'tb-button', type: 'button', text: 'Import images', title: 'Import numbered images as one shot' });
    const picker = h('input', { type: 'file', multiple: true, accept: '.png,.jpg,.jpeg,.webp,.gif,.tif,.tiff,.svg', hidden: true });
    const form = h('form', { class: 'import-shot-form', hidden: true });
    const heading = h('strong', { text: 'Import images as a shot' });
    const count = h('div', { class: 'import-shot-muted' });
    const scene = h('select', { required: true, 'aria-label': 'Scene' });
    const title = h('input', { type: 'text', required: true, maxlength: 200, 'aria-label': 'Shot title' });
    const duration = h('input', { type: 'number', required: true, min: '0.01', max: '3600', step: 'any', 'aria-label': 'Duration in seconds' });
    const progress = h('div', { class: 'import-shot-muted', role: 'status' });
    const errorText = h('div', { class: 'import-shot-error', role: 'alert', hidden: true });
    const submit = h('button', { class: 'btn primary', type: 'submit', text: 'Import shot' });
    const cancel = h('button', { class: 'btn', type: 'button', text: 'Cancel' });
    form.append(heading, count,
      h('label', null, 'Scene', scene),
      h('label', null, 'Shot title', title),
      h('label', null, 'Duration (seconds)', duration),
      progress, errorText,
      h('div', { class: 'import-shot-actions' }, cancel, submit));
    el.append(button, picker, form);
    let files = [];
    let busy = false;
    const sync = () => {
      button.hidden = !state.doc;
      button.disabled = !state.doc || busy;
      if (!state.doc && !busy) form.hidden = true;
    };
    const close = () => { if (!busy) { form.hidden = true; picker.value = ''; } };
    button.addEventListener('click', () => picker.click());
    cancel.addEventListener('click', close);
    picker.addEventListener('change', () => {
      files = [...picker.files];
      if (!files.length) return;
      files.sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' }));
      const selectionError = imageSelectionError(files);
      scene.replaceChildren(...(state.doc?.scenes || []).map((item) => h('option', { value: item.id, text: item.title || item.id })));
      const selected = state.primary?.type === 'scene' ? state.primary.id
        : state.primary?.type === 'shot' ? state.model.shots.get(state.primary.id)?.scene.id : null;
      scene.value = selected || state.doc?.scenes.at(-1)?.id || '';
      title.value = files[0].name.replace(/([\s._-]*\d+)?\.[^.]+$/, '') || 'Image sequence';
      duration.value = String(files.length);
      count.textContent = `${files.length} image${files.length === 1 ? '' : 's'} selected in numeric order`;
      progress.textContent = '';
      errorText.hidden = !selectionError;
      errorText.textContent = selectionError;
      submit.disabled = !!selectionError || !scene.options.length;
      form.hidden = false;
      title.focus();
    });
    form.addEventListener('submit', async (event) => {
      event.preventDefault();
      if (busy || !files.length || !state.project) return;
      const project = state.project;
      const folder = crypto.randomUUID();
      const targetScene = scene.value;
      const afterShot = selectedShot(targetScene);
      const seconds = Number(duration.value);
      if (!Number.isFinite(seconds) || seconds <= 0) { errorText.textContent = 'Enter a positive duration.'; errorText.hidden = false; return; }
      busy = true; submit.disabled = true; cancel.disabled = true; sync();
      errorText.hidden = true;
      try {
        const uploaded = [];
        for (let i = 0; i < files.length; i++) {
          progress.textContent = `Uploading ${i + 1} of ${files.length}: ${files[i].name}`;
          const result = await uploadShotImage(project, folder, files[i]);
          uploaded.push({ name: files[i].name, path: result.path });
        }
        progress.textContent = 'Creating shot…';
        const result = await importShot(project, { scene: targetScene, title: title.value.trim(), duration: seconds, afterShot, files: uploaded });
        if (result) {
          const shotId = (result.ids || []).find((id) => id.startsWith('shot-') && state.model.shots.has(id));
          if (shotId) selectOnly('shot', shotId);
          notify.status({ text: `Imported ${files.length} images as one shot.` });
          form.hidden = true;
          picker.value = '';
        }
      } catch (error) {
        errorText.textContent = `${progress.textContent} failed: ${error.message}`;
        errorText.hidden = false;
        notify.error(error, 'Importing images');
      } finally {
        busy = false; submit.disabled = false; cancel.disabled = false; sync();
      }
    });
    for (const event of ['doc', 'loading', 'projectClosed']) ctx.store.on(event, sync);
    sync();
    return {};
  },
});
