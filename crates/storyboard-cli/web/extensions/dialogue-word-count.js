import { h } from '../js/core/dom.js';
import { registerNodeBadge, registerPanel } from '../js/core/registry.js';

export function dialogueWordCount(document, sceneId) {
  const scene = document?.scenes?.find((candidate) => candidate.id === sceneId);
  return (scene?.shots || []).reduce((total, shot) => total + (String(shot.dialogue || '').trim().match(/\S+/g)?.length || 0), 0);
}

registerNodeBadge({
  id: 'sample.dialogue-word-count', title: 'Dialogue words', order: 90,
  render(shot) {
    const words = String(shot.dialogue || '').trim().match(/\S+/g)?.length || 0;
    return words ? { text: `${words}w`, title: `${words} dialogue words`, class: 'dialogue-words' } : null;
  },
});

registerPanel({
  id: 'sample.dialogue-word-count', title: 'Dialogue words', region: 'toolbar', order: 43, dock: { defaultDock: 'bottom', shortcut: 'Mod+Alt+9' },
  mount(el, ctx) {
    const label = h('span', { class: 'tb-muted', text: '' });
    const render = () => {
      const sceneId = ctx.selection.primary()?.type === 'shot' && ctx.store.state.model?.shots.get(ctx.selection.primary().id)?.scene.id;
      label.textContent = sceneId ? `${dialogueWordCount(ctx.store.state.doc, sceneId)} dialogue words` : '';
    };
    ctx.store.on('doc', render); ctx.store.on('selection', render); render(); el.append(label);
    return {};
  },
});
