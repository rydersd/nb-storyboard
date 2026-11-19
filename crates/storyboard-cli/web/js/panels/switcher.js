// The project button and the project switcher (Cmd/Ctrl+O).

import { closeProject, createProject, openProject, refreshWorkspace } from '../core/api.js';
import { $, els, h } from '../core/dom.js';
import { closePopover, currentPopover, openPopover } from '../core/popover.js';
import { registerCommand, registerPanel } from '../core/registry.js';
import { state } from '../core/store.js';
import { clamp, fmtClock, relTime } from '../core/util.js';

export function openSwitcher() {
  if (currentPopover()?.switcher) { closePopover(); return; }
  const filter = h('input', { type: 'search', placeholder: 'Filter projects', 'aria-label': 'Filter projects' });
  const cards = h('div', { class: 'cards', role: 'listbox', 'aria-label': 'Projects' });
  const newButton = h('button', { class: 'btn', type: 'button', text: 'New project' });
  const closeButton = h('button', { class: 'btn', type: 'button', text: 'Close project' });
  const panel = h('div', { class: 'switcher', role: 'dialog', 'aria-label': 'Projects' },
    h('div', { class: 'switcher-actions' }, newButton, closeButton,
      h('span', { class: 'switcher-save-note', text: 'Edits save automatically.' })), filter, cards);
  newButton.addEventListener('click', openNewProjectForm);
  closeButton.addEventListener('click', async () => { if (await closeProject()) closePopover(); });
  let active = 0;
  let list = [];
  const refresh = () => {
    closeButton.hidden = !state.project;
    const projects = state.workspace?.projects || [];
    const query = filter.value.trim().toLowerCase();
    list = projects.filter((p) => !query || `${p.title} ${p.project}`.toLowerCase().includes(query));
    active = clamp(active, 0, Math.max(0, list.length - 1));
    cards.replaceChildren();
    if (!projects.length) { cards.append(h('div', { class: 'none', text: `No projects in ${state.workspace?.root || 'this workspace'}. Choose New project.` })); return; }
    if (!list.length) { cards.append(h('div', { class: 'none', text: 'No project matches.' })); return; }
    list.forEach((p, index) => {
      const last = p.last_author ? (p.last_author === 'human' ? 'you' : p.last_author.replace(/^agent:/, '')) : null;
      const card = h('button', {
        class: `card${index === active ? ' active' : ''}${p.project === state.project ? ' current' : ''}`, type: 'button', role: 'option',
        'aria-selected': String(index === active), 'aria-label': `${p.title || p.project}, ${p.scene_count} scenes, ${p.shot_count} shots`,
      },
      h('div', { class: 'c-thumb' }, p.thumbnail ? h('img', { src: p.thumbnail, alt: '', loading: 'lazy', decoding: 'async' }) : null),
      h('div', { class: 'c-body' },
        h('div', { class: 'c-title', text: p.title || p.project }),
        h('div', { class: 'c-line', text: `${p.scene_count} scene${p.scene_count === 1 ? '' : 's'}, ${p.shot_count} shot${p.shot_count === 1 ? '' : 's'}, ${fmtClock(p.duration, p.fps)}` }),
        h('div', { class: 'c-line', text: [p.unresolved_stickies ? `${p.unresolved_stickies} open note${p.unresolved_stickies === 1 ? '' : 's'}` : null, last ? `last edit by ${last}` : null, p.modified ? relTime(p.modified) : null].filter(Boolean).join(', ') }),
        p.problem ? h('div', { class: 'c-line problem', text: p.problem }) : null));
      card.addEventListener('click', () => { closePopover(); openProject(p.project); });
      cards.append(card);
    });
  };
  filter.addEventListener('input', () => { active = 0; refresh(); });
  filter.addEventListener('keydown', (event) => {
    event.stopPropagation();
    const columns = Math.max(1, Math.round(cards.clientWidth / 210));
    if (event.key === 'ArrowRight') { active = Math.min(list.length - 1, active + 1); refresh(); event.preventDefault(); }
    else if (event.key === 'ArrowLeft') { active = Math.max(0, active - 1); refresh(); event.preventDefault(); }
    else if (event.key === 'ArrowDown') { active = Math.min(list.length - 1, active + columns); refresh(); event.preventDefault(); }
    else if (event.key === 'ArrowUp') { active = Math.max(0, active - columns); refresh(); event.preventDefault(); }
    else if (event.key === 'Enter' && list[active]) { event.preventDefault(); closePopover(); openProject(list[active].project); }
    else if (event.key === 'Escape') { event.preventDefault(); closePopover(); els.projectButton.focus(); }
  });
  const current = (state.workspace?.projects || []).findIndex((p) => p.project === state.project);
  active = Math.max(0, current);
  refresh();
  openPopover(panel, { center: true, anchor: els.projectButton }).switcher = { refresh };
  filter.focus({ preventScroll: true });
  refreshWorkspace();
}

function openNewProjectForm() {
  const title = h('input', { type: 'text', required: true, autofocus: true, 'aria-label': 'Project title' });
  const folder = h('input', { type: 'text', required: true, maxlength: 64, pattern: '[A-Za-z0-9_\\-]+', 'aria-label': 'Project folder' });
  const fps = h('select', { 'aria-label': 'Frames per second' },
    [24, 25, 30, 60].map((value) => h('option', { value, text: String(value) })));
  const width = h('input', { type: 'number', required: true, min: 1, max: 8192, value: 1440, 'aria-label': 'Canvas width' });
  const height = h('input', { type: 'number', required: true, min: 1, max: 8192, value: 900, 'aria-label': 'Canvas height' });
  const submit = h('button', { class: 'btn', type: 'submit', text: 'Create and open' });
  const error = h('div', { class: 'field-error', role: 'alert', hidden: true });
  const form = h('form', { class: 'new-project-form', role: 'dialog', 'aria-label': 'New project' },
    h('h2', { text: 'New project' }),
    h('label', null, 'Title', title),
    h('label', null, 'Project folder', folder),
    h('div', { class: 'new-project-dimensions' },
      h('label', null, 'Frames per second', fps), h('label', null, 'Width', width), h('label', null, 'Height', height)),
    error, submit);
  let folderEdited = false;
  folder.addEventListener('input', () => { folderEdited = true; });
  title.addEventListener('input', () => {
    if (!folderEdited) folder.value = title.value.normalize('NFKD').replace(/[\u0300-\u036f]/g, '')
      .toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 64);
  });
  form.addEventListener('submit', async (event) => {
    event.preventDefault();
    submit.disabled = true;
    error.hidden = true;
    try {
      await createProject({ project: folder.value, title: title.value, fps: Number(fps.value), width: Number(width.value), height: Number(height.value) });
      closePopover();
    } catch (failure) {
      error.textContent = `${failure.message}${failure.hint ? ` ${failure.hint}` : ''}`;
      error.hidden = false;
    } finally { submit.disabled = false; }
  });
  openPopover(form, { center: true, anchor: els.projectButton });
  title.focus({ preventScroll: true });
}

export function renderProjectButton() {
  const doc = state.doc;
  els.projectTitle.textContent = doc ? (doc.title || state.project) : state.workspace ? 'No project' : 'Storyboard';
  document.title = doc ? `${doc.title || state.project} - Storyboard` : 'Storyboard';
  els.spinner.hidden = !state.loading;
}

registerPanel({
  id: 'switcher', title: 'Project', region: 'toolbar', order: 10, dock: { defaultDock: 'left', shortcut: 'Mod+Alt+6' },
  mount(el, ctx) {
    el.innerHTML = `<button id="project-button" class="tb-button project-button" aria-haspopup="dialog" aria-expanded="false" title="Switch project (Cmd/Ctrl+O)">
      <span id="project-title" class="project-title">Storyboard</span>
      <span id="project-spinner" class="spinner" hidden></span>
      <svg class="chev" viewBox="0 0 12 12" aria-hidden="true"><path d="M3 4.5l3 3 3-3" fill="none" stroke="currentColor" stroke-width="1.5"/></svg>
    </button>`;
    els.projectButton = $('#project-button', el);
    els.projectTitle = $('#project-title', el);
    els.spinner = $('#project-spinner', el);
    els.projectButton.addEventListener('click', openSwitcher);
    for (const event of ['doc', 'loading', 'workspace', 'projectClosed']) ctx.store.on(event, renderProjectButton);
    ctx.store.on('workspace', () => currentPopover()?.switcher?.refresh());
    return {};
  },
});

registerCommand({
  id: 'project.switch', title: 'Switch project', keys: ['Mod+o'], global: true, needsDoc: false, run: () => openSwitcher(),
  keymap: { order: 180, keys: 'Cmd/Ctrl+O', label: 'Switch project' },
});
