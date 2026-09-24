import { createSignal } from 'solid-js';

export type Theme = 'light' | 'dark';

const KEY = 'ghv:theme';
const dark = matchMedia('(prefers-color-scheme: dark)');

function stored(): Theme | null {
  try {
    const t = localStorage.getItem(KEY);
    return t === 'light' || t === 'dark' ? t : null;
  } catch {
    return null;
  }
}

const [explicit, setExplicit] = createSignal<Theme | null>(stored());
const [system, setSystem] = createSignal<Theme>(dark.matches ? 'dark' : 'light');
dark.addEventListener('change', (e) => setSystem(e.matches ? 'dark' : 'light'));

export const theme = () => explicit() ?? system();

export function toggleTheme() {
  const next: Theme = theme() === 'dark' ? 'light' : 'dark';
  // Set the attribute first: effects reading computed CSS tokens run as soon as the signal changes.
  document.documentElement.dataset.theme = next;
  setExplicit(next);
  try {
    localStorage.setItem(KEY, next);
  } catch {
    // Storage can be unavailable (private mode); the choice still applies for this session.
  }
}
