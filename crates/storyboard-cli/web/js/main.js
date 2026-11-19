// Entry point. Loads every built-in panel, command, badge and section (each registers itself), mounts
// the registered panels into the page's fixed regions, then connects and opens the last project.
// Interaction design: wiki/product-spec/plans/workbench-ux.md. HTTP contract: workbench-api.md.

import { commit, openProject, optimisticCommit, refreshWorkspace, undo } from './core/api.js';
import { installKeyboard, runCommand } from './core/commands.js';
import { $, els, h } from './core/dom.js';
import { connectEvents, startLive } from './core/live.js';
import { getPanels, guard, setContext } from './core/registry.js';
import { hiddenGate } from './core/panel-catchup.js';
import { installPanelShell, restorePanelShell } from './core/panel-shell.js';
import { clearSelection, emit, notify, on, selectOnly, setConn, state, toggleSelect } from './core/store.js';
import { prefs } from './core/util.js';

import './panels/graph/render.js';
import './panels/review.js';
import './panels/stickies.js';
import './panels/inspector/index.js';
import './panels/inspector/sections.js';
import './panels/switcher.js';
import './panels/paths.js';
import './panels/toolbar.js';
import './panels/search.js';
import './panels/history.js';
import './panels/sources.js';
import './panels/reorder.js';
import './panels/import-shot.js';
import './panels/import-movie.js';
import './panels/replace-movie.js';
import './panels/attach-audio.js';
import './panels/player.js';
import './panels/cut-editor.js';
import './panels/claude-brief.js';
import './panels/connection.js';
import './panels/status.js';
import './panels/conflict.js';
import './panels/keymap.js';
import './panels/view-layout.js';
import './core/actions.js';
import './core/redo.js';
import '../extensions/dialogue-word-count.js';

/** What every panel, command and section gets: read the store, change it only through these. */
const baseContext = {
  store: { state, on },
  selection: {
    get: () => state.sel,
    primary: () => state.primary,
    selectOnly, toggleSelect, clearSelection,
  },
  commit, undo, optimisticCommit,
  commands: { run: runCommand },
  emit,
};

/** Mounts each registered panel, in order, into its region. A failing panel shows its error in place. */
function mountPanels() {
  const panels = getPanels();
  for (const region of document.querySelectorAll('[data-region]')) {
    for (const panel of panels.filter((candidate) => candidate.region === region.dataset.region)) {
      const slot = h('div', { class: 'slot', dataset: { panel: panel.id } });
      region.append(slot);
      const owner = `Panel "${panel.id}"`;
      // Hidden: store events are skipped and the latest of each is replayed when the panel opens.
      const gate = hiddenGate(() => slot.hidden, (call) => guard(owner, slot, call));
      const guarded = gate.wrap;
      const ctx = { ...baseContext, store: { state, on: (event, fn) => on(event, guarded(fn)) } };
      const handle = guard(owner, slot, () => panel.mount(slot, ctx));
      const update = handle?.update ? guarded(handle.update) : null;
      if (update) on('doc', update);
      on('panelOpened', (id) => {
        if (id !== panel.id) return;
        const replayed = gate.flush();
        if (update && !replayed.has(handle.update)) update({ opened: true, current: true });
      });
      if (handle?.visibilityChanged) {
        on('panelVisibility', ({ id, open }) => {
          if (id === panel.id) guard(owner, slot, () => handle.visibilityChanged(open));
        });
      }
    }
  }
}

async function boot() {
  setConn('connecting');
  connectEvents();
  const workspace = await refreshWorkspace();
  if (!workspace) {
    notify.status({ text: 'Could not read the workspace. Retrying.', error: true });
    setTimeout(boot, 3000);
    return;
  }
  await restorePanelShell();
  const last = prefs.get('last-project');
  if (last === false) { notify.status({ text: 'No project open.' }); return; }
  const pick = workspace.projects.find((p) => p.project === last) || workspace.projects[0];
  if (pick) await openProject(pick.project);
  else notify.status({ text: 'No projects yet.' });
}

els.app = $('#app');
setContext(baseContext);
mountPanels();
installPanelShell();
installKeyboard();
startLive();
boot();
