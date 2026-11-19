// The extension seam: panels, commands, node badges and inspector sections register here. No imports, so any module may register at load time.

const panels = [];
const commands = new Map();
const badges = [];
const sections = [];
let ctx = null;

function need(kind, def, fields) {
  if (!def || typeof def !== 'object') throw new TypeError(`${kind}: expected an object`);
  for (const [field, type] of Object.entries(fields)) {
    if (typeof def[field] !== type) throw new TypeError(`${kind} ${def.id || '(no id)'}: ${field} must be a ${type}`);
  }
}
function unique(kind, list, id) {
  if (list.some((item) => item.id === id)) throw new Error(`${kind} ${id} is already registered`);
}
const byOrder = (a, b) => (a.order ?? 0) - (b.order ?? 0);

/**
 * A panel: {id, title, region, order?, dock?: {defaultDock, shortcut}, mount(el, ctx) -> {update?(info), visibilityChanged?(open), unmount?()}}.
 * region is one of the page's data-region names (toolbar, banner, graph, inspector, status, overlay).
 * dock panels are wrapped by the S19 shell and can move to left, right, bottom or hidden. mount()
 * gets an empty element of its own; update() runs after every document change while visible.
 */
export function registerPanel(panel) {
  need('panel', panel, { id: 'string', title: 'string', region: 'string', mount: 'function' });
  unique('panel', panels, panel.id);
  panels.push(panel);
  return panel;
}

/**
 * A command: {id, title, keys?, run(ctx, event?), when?(ctx, event), needsDoc?, global?, keymap?}.
 * keys are combos such as 'a', 'Shift+T', 'Alt+ArrowLeft', 'Mod+z' (Mod is Cmd or Ctrl; 'Mod?' allows
 * either). needsDoc (default true) skips the command with no project open; global runs it even while
 * typing. keymap: {order, keys, label} adds a row to the keyboard list.
 */
export function registerCommand(command) {
  need('command', command, { id: 'string', title: 'string', run: 'function' });
  if (commands.has(command.id)) throw new Error(`command ${command.id} is already registered`);
  commands.set(command.id, { keys: [], needsDoc: true, global: false, ...command });
  return command;
}

/**
 * A node badge: {id, title?, order?, slot?: 'meta'|'top', render(shot, ctx) -> descriptor | descriptor[] | null}.
 * A descriptor is {text, title?, class?, dot?}. ctx carries the store state, the shot's model entry and,
 * while a cut is viewed, its clip numbers for this shot (ctx.clips).
 */
export function registerNodeBadge(badge) {
  need('node badge', badge, { id: 'string', render: 'function' });
  unique('node badge', badges, badge.id);
  badges.push(badge);
  badges.sort(byOrder);
  return badge;
}

/**
 * An inspector section for the selected shot: {id, title, order, pinned?, mount(el, ctx) -> {update?(entry, view), unmount?()}}.
 * ctx is the panel context plus shotId. Sections are mounted once per selected shot, in order.
 */
export function registerInspectorSection(section) {
  need('inspector section', section, { id: 'string', title: 'string', order: 'number', mount: 'function' });
  unique('inspector section', sections, section.id);
  sections.push(section);
  sections.sort(byOrder);
  return section;
}

export const getPanels = () => [...panels].sort(byOrder);
export const getCommands = () => [...commands.values()];
export const getCommand = (id) => commands.get(id) || null;
export const getNodeBadges = () => badges;
export const getInspectorSections = () => [...sections];

export function setContext(value) { ctx = value; }
export const context = () => ctx;

/** Shows an error inside the element that failed, instead of letting it break the rest of the page. */
export function showPanelError(el, owner, error) {
  console.error(`[workbench] ${owner} failed:`, error);
  if (!el) return;
  let box = el.querySelector(':scope > .panel-error');
  if (!box) {
    box = document.createElement('div');
    box.className = 'panel-error';
    box.setAttribute('role', 'alert');
    el.append(box);
  }
  box.textContent = `${owner} failed: ${error?.message || error}`;
}

/** Runs an extension hook in one timed, failure-isolated boundary. */
export function guard(owner, el, fn, onError = null) {
  const start = globalThis.performance?.now?.();
  const failed = (error) => {
    showPanelError(el, owner, error);
    onError?.(error);
  };
  let result;
  try {
    result = fn();
  } catch (error) {
    failed(error);
    return undefined;
  }
  const elapsed = start === undefined ? 0 : globalThis.performance.now() - start;
  if (elapsed > 60) console.error(`[workbench] interaction budget exceeded by ${owner}: ${elapsed.toFixed(1)} ms`);
  let then;
  try {
    then = result?.then;
  } catch (error) {
    failed(error);
    return undefined;
  }
  if (typeof then === 'function') {
    Promise.resolve(result).catch(failed);
    return undefined;
  }
  return result;
}
