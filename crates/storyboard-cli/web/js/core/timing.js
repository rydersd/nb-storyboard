// Minimum shot length that keeps every storyboard panel visible after a media edit.
export function contentDuration(shot, fps, movie = shot.source_movie, audio = shot.source_audio) {
  const movieSpan = movie ? movie.end - movie.start : 0;
  const audioSpan = audio ? audio.end - audio.start : 0;
  const lastPanel = Math.max(0, ...shot.takes.flatMap((take) => take.panels.map((panel) => panel.time)));
  return Math.max(movieSpan, audioSpan, lastPanel + 1 / fps);
}

// An old VO determines duration only when the stored length matches its content.
// A longer stored length is an intentional hold, so replacing the VO keeps it.
export function durationWithReplacedAudio(shot, fps, audio) {
  const nextContent = contentDuration(shot, fps, shot.source_movie, audio);
  if (shot.source_audio && Math.abs(shot.duration - contentDuration(shot, fps)) < 0.5 / fps) {
    return nextContent;
  }
  return Math.max(shot.duration, nextContent);
}

// Normal playback tolerates small decoder drift. A deliberate seek or shot boundary must
// align within half a project frame, even when the jump is shorter than that drift budget.
export function shouldSyncMedia(current, target, fps, exact) {
  return Math.abs(current - target) > (exact ? 0.5 / fps : 0.12);
}

// Keep an end seek inside the selected media range without backing into an earlier frame.
export function clampMediaTime(start, end, wanted) {
  const margin = Math.min(0.001, Math.max(0, end - start) / 2);
  return Math.max(start, Math.min(end - margin, wanted));
}
