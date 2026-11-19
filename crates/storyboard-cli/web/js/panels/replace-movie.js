// Attach a revised MP4 to an existing shot, retaining its storyboard panels as edit references.

import { commit, uploadMovie } from '../core/api.js';
import { h } from '../core/dom.js';
import { registerPanel } from '../core/registry.js';
import { notify, state } from '../core/store.js';
import { contentDuration } from '../core/timing.js';

export function replacementDuration(shot, movieDuration, fps) {
  return contentDuration(shot, fps, { start: 0, end: movieDuration });
}

registerPanel({
  id: 'replace-movie', title: 'Replace movie', region: 'toolbar', order: 34,
  mount(el, ctx) {
    const button = h('button', { class: 'tb-button', type: 'button', text: 'Replace clip' });
    const picker = h('input', { type: 'file', accept: '.mp4,video/mp4', hidden: true });
    el.append(button, picker);
    let busy = false;
    const sync = () => {
      button.hidden = !state.doc;
      button.disabled = busy || state.primary?.type !== 'shot';
      button.title = button.disabled ? 'Select a shot first' : 'Attach a revised MP4 to the selected shot; storyboard panels stay as references';
    };
    button.addEventListener('click', () => picker.click());
    picker.addEventListener('change', async () => {
      const file = picker.files?.[0];
      const shotId = state.primary?.type === 'shot' ? state.primary.id : null;
      const project = state.project;
      if (!file || !shotId || !project) return;
      if (!/\.mp4$/i.test(file.name) || !file.size || file.size > 1024 * 1024 * 1024) {
        notify.status({ text: 'Choose a nonempty MP4 through 1 GB.', error: true });
        picker.value = '';
        return;
      }
      busy = true; sync();
      try {
        const uploaded = await uploadMovie(project, crypto.randomUUID(), file);
        if (state.project !== project) throw new Error('Project changed during movie upload.');
        const shot = state.doc.scenes.flatMap((scene) => scene.shots).find((item) => item.id === shotId);
        if (!shot) throw new Error('Shot changed during movie upload.');
        const movie = { src: uploaded.path, start: 0, end: uploaded.media.duration };
        await commit([{ update_shot: { shot: shotId, patch: {
          source_movie: movie, duration: replacementDuration(shot, movie.end, state.doc.fps),
        } } }], { what: 'Replace movie clip' });
      } catch (error) { notify.error(error, 'Replacing movie clip'); }
      finally { busy = false; picker.value = ''; sync(); }
    });
    for (const event of ['doc', 'selection', 'projectClosed']) ctx.store.on(event, sync);
    sync();
    return {};
  },
});
