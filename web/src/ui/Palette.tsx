import { createMemo, createSignal, For, Show } from 'solid-js';
import { Dialog } from './Dialog';

export interface Command {
  id: string;
  label: string;
  group: string;
  keys?: string;
  run: () => void;
}

/** Lower is better: prefix beats word start beats substring beats scattered letters; null means no match. */
function score(label: string, query: string) {
  const l = label.toLowerCase();
  const q = query.toLowerCase().trim();
  if (!q) return 0;
  if (l.startsWith(q)) return 1;
  if (l.includes(` ${q}`)) return 2;
  if (l.includes(q)) return 3;
  let at = 0;
  for (const c of q) {
    at = l.indexOf(c, at) + 1;
    if (!at) return null;
  }
  return 4;
}

export function Palette(props: { open: boolean; onClose: () => void; commands: Command[] }) {
  let input!: HTMLInputElement;
  const [query, setQuery] = createSignal('');
  const [active, setActive] = createSignal(0);
  const matches = createMemo(() =>
    props.commands
      .map((c, i) => {
        const scores = [score(c.label, query()), score(`${c.group} ${c.label}`, query())].filter((x) => x !== null);
        return { c, s: scores.length ? Math.min(...scores) : null, i };
      })
      .filter((m) => m.s !== null)
      .sort((a, b) => a.s! - b.s! || a.i - b.i)
      .map((m) => m.c),
  );

  const run = (c: Command | undefined) => {
    if (!c) return;
    props.onClose();
    c.run();
  };

  return (
    <Dialog
      open={props.open}
      onClose={props.onClose}
      label="Command palette"
      class="palette"
      onOpen={() => {
        setQuery('');
        setActive(0);
        input.focus();
      }}
    >
      <input
        ref={input}
        class="palette-input"
        role="combobox"
        aria-expanded="true"
        aria-controls="palette-list"
        aria-autocomplete="list"
        aria-activedescendant={matches()[active()] ? `cmd-${matches()[active()].id}` : undefined}
        aria-label="Command"
        placeholder="Type a command…"
        value={query()}
        onInput={(e) => {
          setQuery(e.currentTarget.value);
          setActive(0);
        }}
        onKeyDown={(e) => {
          const n = matches().length;
          if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
            e.preventDefault();
            if (n) setActive((a) => (a + (e.key === 'ArrowDown' ? 1 : n - 1)) % n);
            document.getElementById(`cmd-${matches()[active()]?.id}`)?.scrollIntoView({ block: 'nearest' });
          } else if (e.key === 'Enter') {
            e.preventDefault();
            run(matches()[active()]);
          }
        }}
      />
      <div id="palette-list" class="palette-list" role="listbox" aria-label="Commands">
        <For each={matches()}>
          {(c, i) => (
            // biome-ignore lint/a11y/useFocusableInteractive: combobox pattern; focus stays in the input via aria-activedescendant
            // biome-ignore lint/a11y/useKeyWithClickEvents: arrow keys and Enter are handled by the combobox input
            <div
              id={`cmd-${c.id}`}
              class="palette-option"
              role="option"
              aria-selected={i() === active()}
              onClick={() => run(c)}
              onMouseMove={() => setActive(i())}
            >
              <span class="palette-group">{c.group}</span>
              <span class="palette-label">{c.label}</span>
              <Show when={c.keys}>
                <kbd>{c.keys}</kbd>
              </Show>
            </div>
          )}
        </For>
        <Show when={!matches().length}>
          <p class="palette-empty">No matching commands</p>
        </Show>
      </div>
    </Dialog>
  );
}
