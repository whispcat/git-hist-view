import { createEffect, createMemo, createRoot, createSignal, on } from 'solid-js';
import { frontier, meta } from './session';

const PLAYBACK_SECONDS = 30;
/** Spring time constant: the playhead covers ~95% of the remaining distance in 3 tau. */
const TAU_MS = 70;
/** Gliding further than this would flash through history, so it cuts instead. */
const MAX_GLIDE = 60;
const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)');

const [pos, setPosRaw] = createSignal(0);
const [playing, setPlaying] = createSignal(false);
/** While history streams in, the playhead rides the analyzed edge until the user takes over. */
const [following, setFollowing] = createSignal(true);

export { following, playing, pos };

export const frameCount = () => meta()?.keyframes.length ?? 0;

/** Keyframes whose commit has been analyzed. */
export const readyFrames = createRoot(() =>
  createMemo(() => {
    const k = meta()?.keyframes;
    if (!k) return 0;
    const f = frontier();
    let lo = 0;
    let hi = k.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (k[mid] < f) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }),
);

const maxPos = () => Math.max(0, readyFrames() - 1);
const clamp = (p: number) => Math.min(Math.max(p, 0), maxPos());

/** Commit index shown at the current (possibly fractional) keyframe position. */
export const commitAt = (p = pos()) => meta()?.keyframes[Math.round(p)] ?? 0;

/** Moves the playhead directly, e.g. while dragging; may land between keyframes. */
export function seek(p: number) {
  stopSettling();
  setFollowing(false);
  setPosRaw(clamp(p));
}

/** Pauses and settles on the next keyframe, so the views never rest mid-transition. */
export function pause() {
  if (!playing()) return;
  setPlaying(false);
  settle(Math.ceil(pos() - 1e-6));
}

/** Fractional keyframe position of a (fractional) commit index. */
export function posOfCommit(c: number) {
  const k = meta()?.keyframes;
  if (!k || k.length < 2) return 0;
  let lo = 0;
  let hi = k.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (k[mid] <= c) lo = mid;
    else hi = mid - 1;
  }
  return lo === k.length - 1 ? lo : lo + (c - k[lo]) / (k[lo + 1] - k[lo]);
}

/** Fractional commit index at a (fractional) keyframe position. */
export function commitOfPos(p: number) {
  const k = meta()?.keyframes;
  if (!k?.length) return 0;
  const i = Math.floor(p);
  return i >= k.length - 1 ? k[k.length - 1] : k[i] + (p - i) * (k[i + 1] - k[i]);
}

let spring = 0;
let target: number | null = null;

function stopSettling() {
  cancelAnimationFrame(spring);
  target = null;
}

/**
 * Glides the playhead to a keyframe with an exponential spring. Retargeting mid-flight keeps the current
 * velocity profile, so rapid key repeats read as one continuous motion instead of restarting an ease.
 */
export function settle(to: number) {
  setFollowing(false);
  const goal = clamp(Math.round(to));
  if (reducedMotion.matches || Math.abs(goal - pos()) > MAX_GLIDE) {
    stopSettling();
    return setPosRaw(goal);
  }
  if (target !== null) {
    target = goal;
    return;
  }
  target = goal;
  let last = performance.now();
  const tick = (now: number) => {
    if (target === null) return;
    const p = pos();
    const next = p + (target - p) * (1 - Math.exp(-(now - last) / TAU_MS));
    last = now;
    if (Math.abs(target - next) < 0.002) {
      setPosRaw(target);
      target = null;
    } else {
      setPosRaw(next);
      spring = requestAnimationFrame(tick);
    }
  };
  spring = requestAnimationFrame(tick);
}

export function stepBy(frames: number) {
  setPlaying(false);
  settle((target ?? Math.round(pos())) + frames);
}

export const toStart = () => stepBy(-Infinity);
export const toEnd = () => stepBy(Infinity);

export function togglePlay() {
  if (playing()) return pause();
  stopSettling();
  setFollowing(false);
  if (pos() >= maxPos()) setPosRaw(0);
  setPlaying(true);
  let last = performance.now();
  const tick = (now: number) => {
    if (!playing()) return;
    const rate = Math.max(frameCount() / PLAYBACK_SECONDS, 2);
    const next = pos() + ((now - last) / 1000) * rate;
    last = now;
    setPosRaw(Math.min(next, maxPos()));
    if (next >= frameCount() - 1) setPlaying(false);
    else requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}

createRoot(() => {
  createEffect(
    on(meta, () => {
      setPlaying(false);
      setFollowing(true);
      setPosRaw(0);
    }),
  );
  createEffect(() => {
    if (following() && !playing()) setPosRaw(maxPos());
  });
});
