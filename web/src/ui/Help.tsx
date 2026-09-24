import { For } from 'solid-js';
import { Dialog } from './Dialog';

const mod = /Mac|iPhone|iPad/.test(navigator.platform) ? '⌘' : 'Ctrl';

const SECTIONS: [string, [string[], string][]][] = [
  [
    'General',
    [
      [['/'], 'Focus the repository box'],
      [[mod, 'K'], 'Command palette'],
      [['T'], 'Switch light / dark'],
      [['?'], 'This help'],
    ],
  ],
  [
    'Timeline',
    [
      [['Space'], 'Play or pause'],
      [['←', '→'], 'Step one keyframe'],
      [['Shift', '←/→'], 'Step ten keyframes'],
      [['Home', 'End'], 'First / last keyframe'],
    ],
  ],
  [
    'Views',
    [
      [['1', '2', '3'], 'Treemap, coupling, dependencies'],
      [['C'], 'Cycle treemap colors'],
      [['Arrows'], 'Move between cells or nodes (view focused)'],
      [['Enter'], 'Open a folder or select a node'],
      [['Esc'], 'Go up, close a folder, or clear'],
    ],
  ],
];

export function Help(props: { open: boolean; onClose: () => void }) {
  return (
    <Dialog open={props.open} onClose={props.onClose} label="Keyboard shortcuts" class="help">
      <h2 class="dialog-title">Keyboard shortcuts</h2>
      <div class="help-grid">
        <For each={SECTIONS}>
          {([title, rows]) => (
            <section>
              <h3>{title}</h3>
              <dl>
                <For each={rows}>
                  {([keys, what]) => (
                    <>
                      <dt>
                        <For each={keys}>{(k) => <kbd>{k}</kbd>}</For>
                      </dt>
                      <dd>{what}</dd>
                    </>
                  )}
                </For>
              </dl>
            </section>
          )}
        </For>
      </div>
      <form method="dialog" class="actions">
        <button type="submit" class="btn">
          Close
        </button>
      </form>
    </Dialog>
  );
}
