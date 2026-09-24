import { For, type JSX } from 'solid-js';
import { fullPath } from '../state/paths';
import { meta, requestCoupling } from '../state/session';
import { commitAt } from '../state/timeline';
import { type CouplingWindow, WINDOW_DAYS, WINDOWS } from '../state/url';
import type { GraphData } from '../worker/protocol';
import { formatDate, formatNumber } from './format';
import { GraphView, PanelClose } from './GraphView';
import { createSegments } from './segments';

const WINDOW_LABELS: Record<CouplingWindow, string> = { '3m': '3 mo', '6m': '6 mo', '1y': '1 yr', '2y': '2 yr' };
export const WINDOW_TEXT: Record<CouplingWindow, string> = { '3m': '3 months', '6m': '6 months', '1y': 'year', '2y': '2 years' };

const radius = (commits: number) => (commits > 0 ? Math.min(3 + 1.6 * Math.sqrt(commits), 16) : 0);

export function CouplingView(props: { window: CouplingWindow; onWindow: (w: CouplingWindow) => void; controls: JSX.Element; table: boolean }) {
  const { segment, t } = createSegments<GraphData>({
    family: () => (meta() ? String(WINDOW_DAYS[props.window]) : null),
    fetch: (k) => requestCoupling({ k, windowDays: WINDOW_DAYS[props.window] }),
  });

  return (
    <GraphView
      segment={segment}
      t={t}
      radius={radius}
      directed={false}
      roleDescription="co-change graph"
      ariaLabel={`Files that changed together up to ${meta() ? formatDate(meta()!.times[commitAt()]) : ''}. Arrow keys move between files, Enter selects one to list its partners, Escape clears.`}
      loadingText="Finding files that change together…"
      emptyText="No files changed together at least 3 times in this window. Try a longer window or a later point in history."
      bar={
        <>
          <span class="view-title">Files changed together in the last {WINDOW_TEXT[props.window]}</span>
          {props.controls}
          <fieldset class="modes">
            <legend class="sr-only">Time window</legend>
            <For each={WINDOWS}>
              {(w) => (
                <button type="button" class="mode" aria-pressed={props.window === w} onClick={() => props.onWindow(w)}>
                  {WINDOW_LABELS[w]}
                </button>
              )}
            </For>
          </fieldset>
        </>
      }
      table={props.table}
      tableOf={(g) => {
        const rows: (string | number)[][] = [];
        for (let e = 0; e < g.data.ends.length / 2; e++) {
          const count = g.data.count[e * 2 + g.side];
          if (count)
            rows.push([
              fullPath(g.pathOf(g.data.ends[e * 2])),
              fullPath(g.pathOf(g.data.ends[e * 2 + 1])),
              count,
              `${Math.round(g.data.weight[e * 2 + g.side] * 100)}%`,
            ]);
        }
        rows.sort((a, b) => (b[2] as number) - (a[2] as number));
        return {
          caption: `Files changed together in the last ${WINDOW_TEXT[props.window]} before ${formatDate(meta()!.times[commitAt()])}`,
          columns: [{ label: 'File' }, { label: 'Changes with' }, { label: 'Commits together', numeric: true }, { label: 'Share of commits', numeric: true }],
          rows,
        };
      }}
      describe={(i, g) => {
        const ps = g.links(i);
        const top = ps
          .slice(0, 2)
          .map((p) => g.label(p.node))
          .join(' and ');
        return `${g.label(i)}, ${formatNumber(g.data.size[i * 2 + g.side])} commits, changes with ${ps.length} files${top ? `, mostly ${top}` : ''}`;
      }}
      tooltip={(i, g) => (
        <>
          <div class="tip-path mono">{fullPath(g.pathOf(i))}</div>
          <div class="tip-meta">
            {formatNumber(g.data.size[i * 2 + g.side])} commits · changes with {g.links(i).length} files
          </div>
        </>
      )}
      panel={(i, g) => (
        <aside class="graph-panel" aria-label="Coupled files">
          <PanelClose onClose={() => g.select(null)} />
          <p class="tip-path mono">{fullPath(g.pathOf(i))}</p>
          <p class="tip-meta">
            {formatNumber(g.data.size[i * 2 + g.side])} commits in the last {WINDOW_TEXT[props.window]}
          </p>
          <ol class="partners">
            <For each={g.links(i)}>
              {(p) => (
                <li>
                  <button type="button" class="partner" onClick={() => g.select(g.data.ids[p.node])}>
                    <span class="partner-name mono">{g.label(p.node)}</span>
                    <span class="partner-meta">
                      {p.count}× · {Math.round(p.weight * 100)}%
                    </span>
                  </button>
                </li>
              )}
            </For>
          </ol>
          <p class="tip-meta partner-help">× commits together · % of either file’s commits</p>
        </aside>
      )}
    />
  );
}
