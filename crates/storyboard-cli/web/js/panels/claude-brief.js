// Source-bound Claude brief and a bounded local storyboard MCP run.

import { h } from '../core/dom.js';
import { api, postJson, refreshProject } from '../core/api.js';
import { registerPanel } from '../core/registry.js';
import { notify, state } from '../core/store.js';
import { revisionBrief } from '../core/revision-brief.js';

registerPanel({
  id: 'claude-brief', title: 'Claude brief', region: 'toolbar', order: 35,
  mount(el, ctx) {
    const button = h('button', { class: 'tb-button', type: 'button', text: 'Copy Claude brief', title: 'Copy selected shots, source ranges, notes and cut order' });
    const open = h('button', { class: 'tb-button', type: 'button', text: 'Run Claude', title: 'Ask Claude to edit this storyboard through MCP' });
    const instruction = h('textarea', { rows: '5', placeholder: 'Describe the storyboard changes you want Claude to make.' });
    const status = h('div', { class: 'import-shot-muted', text: 'Claude can edit the storyboard and render an MP4 from its current boards or source footage. New footage needs an external generator.' });
    const output = h('pre', { class: 'claude-run-output', hidden: true });
    const exports = h('div', { class: 'import-shot-muted' });
    const start = h('button', { type: 'submit', text: 'Start run' });
    const stop = h('button', { type: 'button', text: 'Stop run', hidden: true });
    const close = h('button', { type: 'button', text: 'Close' });
    const form = h('form', { class: 'import-shot-form claude-run-form', hidden: true },
      h('strong', { text: 'Run Claude on storyboard' }),
      h('label', null, 'Instruction', instruction),
      status, exports, output,
      h('div', { class: 'import-shot-actions' }, close, stop, start));
    el.append(button, open, form);
    let active = null;
    let timer = null;
    const selectedShots = () => {
      const selected = [...state.sel.shots];
      if (state.primary?.type === 'shot' && !selected.includes(state.primary.id)) selected.push(state.primary.id);
      return selected;
    };
    const showJob = (job) => {
      active = job.status === 'none' ? null : job;
      const running = job.status === 'running';
      const shotChanges = job.shot_changes || [];
      const cutChanges = job.cut_changes || [];
      const movieChanges = job.movie_changes || [];
      const newExports = job.new_exports || [];
      const saved = job.project_changed
        ? ` Saved project edits.${shotChanges.length ? ` ${shotChanges.length} shot${shotChanges.length === 1 ? '' : 's'} changed.` : ''}${cutChanges.length ? ` ${cutChanges.length} cut${cutChanges.length === 1 ? '' : 's'} changed.` : ''}${movieChanges.length ? ` ${movieChanges.length} movie attachment change${movieChanges.length === 1 ? '' : 's'}.` : ''}`
        : ' No saved project edits.';
      status.textContent = running ? `Claude is running on ${job.project}.` : job.status === 'none'
        ? 'Claude can edit the storyboard and render an MP4 from its current boards or source footage. New footage needs an external generator.'
        : `Claude ${job.status}.${job.project_error ? ' Project could not be reopened.' : saved}${newExports.length ? ` ${newExports.length} MP4 export${newExports.length === 1 ? '' : 's'} added during the run.` : ''}`;
      exports.replaceChildren(...newExports.map((file) => h('a', {
        href: `/api/export.mp4?project=${encodeURIComponent(job.project)}&file=${encodeURIComponent(file)}`,
        download: file, text: `Download ${file}`,
      })));
      const changedMovies = movieChanges.map((change) => `${change.shot}: ${change.source_movie?.src || 'movie removed'}`);
      output.textContent = [job.output, job.error, job.project_error,
        shotChanges.length ? `Shot changes:\n${shotChanges.map((change) => `${change.id}: ${change.kind}`).join('\n')}` : '',
        cutChanges.length ? `Cut changes:\n${cutChanges.map((change) => `${change.id}: ${change.kind}`).join('\n')}` : '',
        changedMovies.length ? `Movie changes:\n${changedMovies.join('\n')}` : ''].filter(Boolean).join('\n\n');
      output.hidden = !output.textContent;
      start.hidden = running;
      stop.hidden = !running;
      instruction.disabled = running;
    };
    const poll = async () => {
      if (!state.project) return;
      const project = state.project;
      try {
        const job = await api(`/api/claude-job?project=${encodeURIComponent(project)}`);
        if (state.project !== project) return;
        const wasRunning = active?.status === 'running';
        showJob(job);
        if (wasRunning && job.status !== 'running') {
          if (job.project_changed) await refreshProject();
          notify.status({ text: `Claude ${job.status}. Open Run Claude to review the result.` });
        }
      } catch (error) { notify.error(error, 'Checking Claude run'); }
    };
    timer = setInterval(() => { if (active?.status === 'running') poll(); }, 1000);
    open.addEventListener('click', () => { form.hidden = !form.hidden; if (!form.hidden) poll(); });
    close.addEventListener('click', () => { form.hidden = true; });
    stop.addEventListener('click', async () => {
      if (!active || !state.project) return;
      try { await postJson('/api/claude-cancel', { project: state.project, id: active.id }); await poll(); }
      catch (error) { notify.error(error, 'Stopping Claude'); }
    });
    form.addEventListener('submit', async (event) => {
      event.preventDefault();
      if (!state.doc || !state.project || !instruction.value.trim()) return;
      start.disabled = true;
      try {
        const job = await postJson('/api/claude-job', {
          project: state.project, base_version: state.version,
          instruction: instruction.value.trim(),
          brief: revisionBrief(state.project, state.doc, selectedShots(), state.workspace?.root_path, state.workspace?.cli_path),
        });
        showJob(job);
        notify.status({ text: 'Claude storyboard run started.' });
      } catch (error) { notify.error(error, 'Starting Claude'); }
      finally { start.disabled = false; }
    });
    button.addEventListener('click', async () => {
      if (!state.doc || !state.project) return;
      const selected = selectedShots();
      try {
        await navigator.clipboard.writeText(revisionBrief(state.project, state.doc, selected, state.workspace?.root_path, state.workspace?.cli_path));
        notify.status({ text: `Copied ${selected.length ? `${selected.length} selected shot${selected.length === 1 ? '' : 's'}` : 'the project'} for Claude.` });
      } catch (error) { notify.error(error, 'Copying Claude brief'); }
    });
    const sync = () => { button.hidden = !state.doc; open.hidden = !state.doc; if (!state.doc) { form.hidden = true; showJob({ status: 'none' }); } };
    for (const event of ['doc', 'projectClosed']) ctx.store.on(event, sync);
    sync();
    return { destroy() { clearInterval(timer); } };
  },
});
