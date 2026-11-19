// Attach a voice-over to the selected shot and extend its default duration to cover it.

import { commit, uploadAudio } from '../core/api.js';
import { h } from '../core/dom.js';
import { registerPanel } from '../core/registry.js';
import { notify, state } from '../core/store.js';
import { durationWithReplacedAudio } from '../core/timing.js';

registerPanel({
  id: 'attach-audio', title: 'Attach audio', region: 'toolbar', order: 35,
  mount(el, ctx) {
    const button = h('button', { class: 'tb-button', type: 'button', text: 'Attach VO' });
    const picker = h('input', { type: 'file', accept: '.mp3,.wav,.m4a,audio/mpeg,audio/wav,audio/mp4', hidden: true });
    el.append(button, picker);
    let busy = false;
    const sync = () => { button.hidden = !state.doc; button.disabled = busy || state.primary?.type !== 'shot';
      button.title = button.disabled ? 'Select a shot first' : 'Attach MP3, WAV or M4A to selected shot'; };
    button.addEventListener('click', () => picker.click());
    picker.addEventListener('change', async () => {
      const file = picker.files?.[0];
      const shotId = state.primary?.type === 'shot' ? state.primary.id : null;
      const project = state.project;
      if (!file || !shotId || !project) return;
      const ext = file.name.split('.').at(-1)?.toLowerCase();
      if (!['mp3', 'wav', 'm4a'].includes(ext) || !file.size || file.size > 128 * 1024 * 1024) {
        notify.status({ text: 'Choose an MP3, WAV or M4A under 128 MB.', error: true });
        picker.value = '';
        return;
      }
      busy = true; sync();
      try {
        const uploaded = await uploadAudio(project, crypto.randomUUID(), ext, file);
        if (state.project !== project) throw new Error('Project changed during audio upload.');
        const shot = state.doc.scenes.flatMap((scene) => scene.shots).find((item) => item.id === shotId);
        if (!shot) throw new Error('Shot changed during audio upload.');
        const audio = { src: uploaded.path, start: 0, end: uploaded.media.duration };
        await commit([{ update_shot: { shot: shotId, patch: {
          source_audio: audio, duration: durationWithReplacedAudio(shot, state.doc.fps, audio),
        } } }], { what: 'Attach voice-over' });
      } catch (error) { notify.error(error, 'Attaching voice-over'); }
      finally { busy = false; picker.value = ''; sync(); }
    });
    for (const event of ['doc', 'selection', 'projectClosed']) ctx.store.on(event, sync);
    sync();
    return {};
  },
});
