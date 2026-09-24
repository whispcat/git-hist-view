import { type Accessor, createEffect, createMemo, createSignal, on } from 'solid-js';
import { meta } from '../state/session';
import { frameCount, pos, readyFrames } from '../state/timeline';

const CACHE_LIMIT = 16;

/**
 * Keyframe segments for a view: fetched around the playhead in both directions, re-fetched while
 * held for analysis, and never blanking while one is in flight.
 */
export function createSegments<T extends { k: number; held: boolean }>(params: {
  /** Identifies everything besides the keyframe that a segment depends on (root, size, window…). */
  family: Accessor<string | null>;
  fetch: (k: number) => Promise<T | null>;
}) {
  const cache = new Map<string, T>();
  const inflight = new Set<string>();
  const [version, setVersion] = createSignal(0);
  const k0 = () => Math.min(Math.floor(pos()), Math.max(0, frameCount() - 1));
  const key = (family: string, k: number) => `${family}|${k}`;

  createEffect(on(meta, () => cache.clear()));

  createEffect(() => {
    const family = params.family();
    const ready = readyFrames();
    if (family === null || !meta()) return;
    for (const k of [k0(), k0() + 1, k0() - 1, k0() + 2]) {
      const id = key(family, k);
      if (k < 0 || k >= ready || inflight.has(id) || (cache.has(id) && !cache.get(id)!.held)) continue;
      inflight.add(id);
      params.fetch(k).then((data) => {
        inflight.delete(id);
        if (!data) return;
        cache.set(id, data);
        if (cache.size > CACHE_LIMIT) cache.delete(cache.keys().next().value!);
        setVersion((v) => v + 1);
      });
    }
  });

  // While the segment under the playhead is in flight, the previous keyframe's segment at its end state is
  // exactly the same picture; failing that (or while a new root/size/expansion loads), keep the last one shown.
  const shown = createMemo<{ repo: unknown; data: T } | null>((prev) => {
    version();
    const family = params.family();
    const repo = meta();
    if (family === null || !repo) return null;
    const data = cache.get(key(family, k0())) ?? cache.get(key(family, k0() - 1));
    if (data) return { repo, data };
    return prev?.repo === repo ? prev : null;
  }, null);

  const segment = () => shown()?.data ?? null;
  const t = () => {
    const d = segment();
    if (!d) return 0;
    if (d.k === k0()) return d.held ? 0 : Math.min(Math.max(pos() - d.k, 0), 1);
    return d.k < k0() ? 1 : 0;
  };
  return { segment, t };
}
