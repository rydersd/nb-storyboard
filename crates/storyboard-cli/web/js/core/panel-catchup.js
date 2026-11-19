// S19: a hidden panel does no work, and catches up when it is opened again. Store events that arrive
// while the panel is hidden are not run; the latest payload of each subscription is kept and replayed
// when the panel opens, so panels that only subscribe (no update()) still show the current state.
export function hiddenGate(isHidden, run) {
  const missed = new Map();
  return {
    wrap: (fn) => (payload) => {
      if (isHidden()) { missed.delete(fn); missed.set(fn, payload); return undefined; }
      return run(() => fn(payload));
    },
    /** Replays every missed subscription once, in the order of their latest events; returns the replayed functions. */
    flush() {
      const pending = [...missed];
      missed.clear();
      for (const [fn, payload] of pending) run(() => fn(payload));
      return new Set(pending.map(([fn]) => fn));
    },
  };
}
