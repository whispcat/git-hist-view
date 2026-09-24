import { createSignal } from 'solid-js';
import type { PathsDelta } from '../worker/protocol';

/** Main-thread mirror of the engine's append-only path table. */
let names: string[] = [];
let parents: number[] = [];
const [version, setVersion] = createSignal(0);

/** Bumps when names arrive, for UI that renders paths it may have asked about too early. */
export const pathsVersion = version;

export function resetPaths() {
  names = [];
  parents = [];
  setVersion((v) => v + 1);
}

export function appendPaths(delta: PathsDelta) {
  if (delta.start !== names.length) return;
  names.push(...delta.names);
  for (const p of delta.parents) parents.push(p);
  setVersion((v) => v + 1);
}

export const pathName = (id: number) => names[id] ?? '';
export const pathParent = (id: number) => parents[id] ?? 0;

export function fullPath(id: number) {
  const parts: string[] = [];
  for (let at = id; at !== 0 && at < names.length; at = parents[at]) parts.push(names[at]);
  return parts.reverse().join('/');
}

/** Ancestors from the repository root down to `id`, inclusive. */
export function pathChain(id: number) {
  const chain: number[] = [];
  for (let at = id; at !== 0 && at < names.length; at = parents[at]) chain.push(at);
  return chain.reverse();
}

let byPath: Map<string, number> | null = null;
let byPathVersion = -1;

/** Path id for a repository path, once the engine has sent its name; tracks `pathsVersion`. */
export function findPath(path: string) {
  if (byPathVersion !== version()) {
    byPath = new Map(names.map((_, id) => [fullPath(id), id]));
    byPathVersion = version();
  }
  return byPath!.get(path);
}
