import { For, Show } from 'solid-js';
import { label, progress, remote } from '../state/session';
import type { Phase } from '../worker/protocol';
import { formatBytes, formatNumber } from './format';

const REMOTE: Phase[] = ['refs', 'download', 'index', 'walk'];
const LOCAL: Phase[] = ['read', 'walk'];
const NAMES: Record<Phase, string> = {
  refs: 'Connecting',
  download: 'Downloading history',
  index: 'Indexing objects',
  read: 'Reading repository',
  walk: 'Walking commits',
};

export function Loading() {
  const phases = () => (remote() ? REMOTE : LOCAL);
  const current = () => phases().indexOf(progress()?.phase ?? phases()[0]);
  const detail = (phase: Phase) => {
    const p = progress();
    if (!p || p.phase !== phase) return '';
    if (phase === 'download') return formatBytes(p.done);
    if (p.total) return `${formatNumber(p.done)} / ${formatNumber(p.total)}`;
    return '';
  };
  const fraction = () => {
    const p = progress();
    return p?.total ? p.done / p.total : undefined;
  };
  return (
    <div class="center-panel">
      <div class="panel" aria-busy="true">
        <h2>{label()}</h2>
        <ol class="phases">
          <For each={phases()}>
            {(phase, i) => (
              <li data-state={i() < current() ? 'done' : i() === current() ? 'active' : 'pending'}>
                <span class="dot" aria-hidden="true" />
                <span>{NAMES[phase]}</span>
                <span class="detail">{detail(phase)}</span>
              </li>
            )}
          </For>
        </ol>
        <Show when={fraction() !== undefined}>
          <div
            class="bar"
            role="progressbar"
            aria-label={NAMES[progress()!.phase]}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(fraction()! * 100)}
          >
            <div style={{ width: `${fraction()! * 100}%` }} />
          </div>
        </Show>
      </div>
    </div>
  );
}
