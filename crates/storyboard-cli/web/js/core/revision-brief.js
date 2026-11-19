// A portable, explicit handoff for a storyboard-aware coding agent.

export function revisionBrief(project, doc, selected = [], workspacePath = '', cliPath = '') {
  const chosen = new Set(selected);
  const all = doc.scenes.flatMap((scene) => scene.shots.map((shot) => ({ scene, shot })));
  const shots = chosen.size ? all.filter(({ shot }) => chosen.has(shot.id)) : all;
  const lines = [
    `Storyboard revision: ${doc.title} (project folder: ${project})`,
    ...(workspacePath ? [`Workspace: ${workspacePath}`, `Project path: ${workspacePath.replace(/\/$/, '')}/${project}`] : []),
    ...(cliPath ? [`Storyboard CLI: ${cliPath}`] : []),
    `Project: ${doc.fps} fps, ${doc.format.width}×${doc.format.height}`,
    'Read this project with the storyboard MCP show tool before editing. If MCP is unavailable, run the storyboard CLI shown above with its show command and the project path. Commit changes through storyboard operations so they remain undoable. A cut placement can override its reusable shot voice-over; preserve that distinction when retiming. For a generated replacement MP4, use the storyboard replace_movie MCP tool or replace-movie CLI command with the target shot and generated file path; it probes and copies the clip into the project.',
    '',
    chosen.size ? 'Selected shots:' : 'Shots:',
  ];
  for (const { scene, shot } of shots) {
    lines.push(`- ${scene.title || scene.id} / ${shot.title || shot.id} [${shot.id}], ${shot.duration}s`);
    if (shot.source_movie) {
      const movie = shot.source_movie;
      lines.push(`  Original MP4: ${movie.src}, ${movie.start}s–${movie.end}s`);
    }
    if (shot.source_audio) {
      const audio = shot.source_audio;
      lines.push(`  Attached audio: ${audio.src}, ${audio.start}s–${audio.end}s`);
    }
    if (shot.action) lines.push(`  Action: ${shot.action}`);
    if (shot.dialogue) lines.push(`  Dialogue: ${shot.dialogue}`);
    if (shot.notes) lines.push(`  Revision notes: ${shot.notes}`);
    const take = shot.takes.find((candidate) => candidate.id === shot.selected_take) || shot.takes[0];
    if (take) {
      lines.push(`  Panels: ${take.panels.map((panel) => `${panel.id}@${panel.time}s`).join(', ')}`);
      for (const panel of take.panels) {
        const images = (panel.layers || []).filter((layer) => layer.type === 'image' && layer.src).map((layer) => layer.src);
        if (panel.title || images.length) {
          lines.push(`  ${panel.id}${panel.title ? ` ${panel.title}` : ''}${images.length ? ` image sources: ${images.join(', ')}` : ''}`);
        }
        if (panel.direction) lines.push(`  ${panel.id} direction: ${JSON.stringify(panel.direction)}`);
        for (const layer of panel.layers || []) {
          if (layer.type === 'text' && layer.text) {
            lines.push(`  ${panel.id} text layer${layer.label ? ` ${JSON.stringify(layer.label)}` : ''}: ${JSON.stringify(layer.text)}`);
          }
        }
      }
    }
  }
  if (doc.cuts?.length) {
    lines.push('', 'Cuts:');
    for (const cut of doc.cuts) {
      lines.push(`- ${cut.name || cut.id} [${cut.id}]${doc.active_cut === cut.id ? ' (active)' : ''}`);
      for (const [index, clip] of cut.clips.entries()) {
        const source = clip.source.shot?.shot || `cut:${clip.source.cut?.cut}`;
        lines.push(`  ${index + 1}. ${source} in=${clip.in || 0}f out=${clip.out ?? 'source end'} slot=${clip.slot ?? 'automatic'}`);
        if (clip.source_audio) {
          lines.push(`     Placement VO: ${clip.source_audio.src}, ${clip.source_audio.start}s–${clip.source_audio.end}s (overrides shot VO for this placement)`);
        }
      }
    }
  }
  lines.push('', 'Use the source ranges and revision notes above as the edit targets. Report what changed, any generated media paths, and what still needs review.');
  return lines.join('\n');
}
