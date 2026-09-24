export type View = 'treemap' | 'coupling' | 'deps';
export type ColorMode = 'churn' | 'author' | 'language';

export type CouplingWindow = '3m' | '6m' | '1y' | '2y';

export const VIEWS: View[] = ['treemap', 'coupling', 'deps'];
export const WINDOWS: CouplingWindow[] = ['3m', '6m', '1y', '2y'];
export const WINDOW_DAYS: Record<CouplingWindow, number> = { '3m': 91, '6m': 182, '1y': 365, '2y': 730 };
export const COLOR_MODES: ColorMode[] = ['churn', 'author', 'language'];

interface HashState {
  repo?: string;
  view?: View;
  color?: ColorMode;
  window?: CouplingWindow;
  /** Folder the treemap is opened into, as a repository path. */
  path?: string;
  /** Short sha of the commit shown, stable across commit caps unlike a keyframe index. */
  at?: string;
  cap?: number;
}

export function readHash(): HashState {
  const p = new URLSearchParams(location.hash.slice(1));
  const pick = <T extends string>(key: string, allowed: readonly T[]) => allowed.find((v) => v === p.get(key));
  const cap = Number(p.get('n'));
  return {
    repo: p.get('r') ?? undefined,
    view: pick('v', VIEWS),
    color: pick('c', COLOR_MODES),
    window: pick('w', WINDOWS),
    path: p.get('p')?.replace(/\p{Cc}/gu, '') || undefined,
    at: p.get('t')?.match(/^[0-9a-f]{4,40}$/)?.[0],
    cap: cap > 0 ? cap : undefined,
  };
}

export function writeHash(s: HashState, push = false) {
  const p = new URLSearchParams();
  if (s.repo) p.set('r', s.repo);
  if (s.view) p.set('v', s.view);
  if (s.color) p.set('c', s.color);
  if (s.window) p.set('w', s.window);
  if (s.path) p.set('p', s.path);
  if (s.at) p.set('t', s.at);
  if (s.cap) p.set('n', String(s.cap));
  // URLSearchParams escapes '/', which makes shared links needlessly ugly.
  const hash = `#${p.toString().replaceAll('%2F', '/')}`;
  if (hash === location.hash || (hash === '#' && !location.hash)) return;
  history[push ? 'pushState' : 'replaceState'](null, '', hash === '#' ? location.pathname : hash);
}
