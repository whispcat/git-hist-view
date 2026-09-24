import { createEffect, createSignal, For, type JSX, on, Show } from 'solid-js';
import { fullPath, pathChain, pathName } from '../state/paths';
import { meta, parsing, requestDeps } from '../state/session';
import { commitAt } from '../state/timeline';
import type { GraphData } from '../worker/protocol';
import { announce } from './Announcer';
import { formatDate, formatNumber } from './format';
import { type GraphApi, GraphView, type Link, PanelClose } from './GraphView';
import { createSegments } from './segments';

const radius = (loc: number) => (loc > 0 ? Math.min(3 + Math.sqrt(loc) / 5, 18) : 0);

export function DepsView(props: { controls: JSX.Element; table: boolean }) {
  /** Opened folders; null until the engine picks a starting set that fits on screen. */
  const [expanded, setExpanded] = createSignal<number[] | null>(null);
  createEffect(on(meta, () => setExpanded(null)));

  const { segment, t } = createSegments<GraphData>({
    family: () => (meta() ? (expanded()?.join(',') ?? 'auto') : null),
    fetch: (k) => requestDeps({ k, expanded: expanded() }),
  });
  // Adopt the engine's automatic choice so every keyframe and prefetch shows the same folders.
  createEffect(() => {
    const d = segment();
    if (d && expanded() === null) setExpanded([...d.expanded]);
  });

  const expand = (id: number) => {
    setExpanded([...(expanded() ?? []), id].sort((a, b) => a - b));
    announce(`Opened ${fullPath(id)}`);
  };

  /** Closes a folder along with any folders opened inside it. */
  const collapse = (id: number) => {
    setExpanded((expanded() ?? []).filter((e) => e !== id && !pathChain(e).includes(id)));
    announce(`Closed ${fullPath(id)}`);
  };

  /** The innermost opened folder containing a node, which Escape closes. */
  const openParent = (path: number) => {
    const open = new Set(expanded() ?? []);
    return pathChain(path)
      .slice(0, -1)
      .reverse()
      .find((p) => open.has(p));
  };

  const counts = (links: Link[]) => [links.filter((l) => l.outgoing).length, links.filter((l) => !l.outgoing).length];
  const loc = (i: number, g: GraphApi) => formatNumber(g.data.size[i * 2 + g.side]);

  const list = (title: string, links: Link[], g: GraphApi) => (
    <Show when={links.length}>
      <p class="tip-meta panel-heading">{title}</p>
      <ol class="partners">
        <For each={links}>
          {(l) => (
            <li>
              <button type="button" class="partner" onClick={() => g.select(g.data.ids[l.node])}>
                <span class="partner-name mono">{g.label(l.node)}</span>
                <span class="partner-meta">{l.count}</span>
              </button>
            </li>
          )}
        </For>
      </ol>
    </Show>
  );

  return (
    <GraphView
      segment={segment}
      t={t}
      radius={radius}
      directed
      roleDescription="dependency graph"
      ariaLabel={`Folders and files and what they import, at ${meta() ? formatDate(meta()!.times[commitAt()]) : ''}. Arrow keys move between nodes, Enter opens a folder or selects a file, Escape closes the folder around it.`}
      loadingText={parsing() ? `Reading imports… ${formatNumber(parsing()!.done)} / ${formatNumber(parsing()!.total)} files` : 'Reading imports…'}
      emptyText="No TypeScript, JavaScript, Rust or Python imports between files at this point in history."
      bar={
        <>
          <span class="view-title">What imports what · edges fade in toward the imported file</span>
          <Show when={parsing()} fallback={props.controls}>
            {(p) => (
              <span class="view-notice">
                Reading imports {formatNumber(p().done)} / {formatNumber(p().total)}
              </span>
            )}
          </Show>
          <button type="button" class="mode" onClick={() => setExpanded(null)} title="Let the view choose which folders to open">
            Auto layout
          </button>
        </>
      }
      table={props.table}
      tableOf={(g) => {
        const rows: (string | number)[][] = [];
        for (let e = 0; e < g.data.ends.length / 2; e++) {
          const count = g.data.count[e * 2 + g.side];
          if (count) rows.push([g.label(g.data.ends[e * 2]), g.label(g.data.ends[e * 2 + 1]), count]);
        }
        rows.sort((a, b) => (b[2] as number) - (a[2] as number));
        return {
          caption: `Imports between files and folders at ${formatDate(meta()!.times[commitAt()])}`,
          columns: [{ label: 'Importer' }, { label: 'Imports from' }, { label: 'Import statements', numeric: true }],
          rows,
        };
      }}
      describe={(i, g) => {
        const [out, inn] = counts(g.links(i));
        return `${g.label(i)}, ${g.data.dir[i] ? 'folder, ' : ''}${loc(i, g)} lines, imports ${out}, imported by ${inn}`;
      }}
      tooltip={(i, g) => {
        const [out, inn] = counts(g.links(i));
        return (
          <>
            <div class="tip-path mono">
              {fullPath(g.pathOf(i))}
              {g.data.dir[i] ? '/' : ''}
            </div>
            <div class="tip-meta">
              {loc(i, g)} lines · imports {out} · imported by {inn}
            </div>
            <Show when={g.data.dir[i]}>
              <div class="tip-meta">Double-click to open</div>
            </Show>
          </>
        );
      }}
      panel={(i, g) => {
        const links = g.links(i);
        const parent = openParent(g.pathOf(i));
        return (
          <aside class="graph-panel" aria-label="Dependencies">
            <PanelClose onClose={() => g.select(null)} />
            <p class="tip-path mono">
              {fullPath(g.pathOf(i))}
              {g.data.dir[i] ? '/' : ''}
            </p>
            <p class="tip-meta">{loc(i, g)} lines</p>
            {list(
              'Imports',
              links.filter((l) => l.outgoing),
              g,
            )}
            {list(
              'Imported by',
              links.filter((l) => !l.outgoing),
              g,
            )}
            <div class="actions panel-actions">
              <Show when={g.data.dir[i]}>
                <button type="button" class="btn" onClick={() => expand(g.pathOf(i))}>
                  Open folder
                </button>
              </Show>
              <Show when={parent}>
                {(p) => (
                  <button type="button" class="btn" onClick={() => collapse(p())}>
                    Close {pathName(p())}/
                  </button>
                )}
              </Show>
            </div>
          </aside>
        );
      }}
      onActivate={(i, g) => {
        if (!g.data.dir[i]) return false;
        expand(g.pathOf(i));
        return true;
      }}
      onBack={(focused, g) => {
        const all = expanded() ?? [];
        const target = focused >= 0 ? openParent(g.pathOf(focused)) : all.at(-1);
        if (target === undefined) return false;
        collapse(target);
        g.select(null);
        return true;
      }}
    />
  );
}
