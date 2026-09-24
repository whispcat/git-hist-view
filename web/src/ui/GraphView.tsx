import { select } from 'd3-selection';
import { zoom, zoomIdentity } from 'd3-zoom';
import { type Accessor, batch, createEffect, createMemo, createSignal, type JSX, on, onCleanup, onMount, Show } from 'solid-js';
import { GraphRenderer, type Radius, type View } from '../render/graph';
import { drawGraphOverlay, nodeScreen, pickNode } from '../render/graphOverlay';
import { palette as makePalette, OTHERS } from '../render/palette';
import { pathName, pathParent, pathsVersion } from '../state/paths';
import { commitCount, meta } from '../state/session';
import { theme } from '../state/theme';
import type { GraphData } from '../worker/protocol';
import { announce } from './Announcer';
import { DataTable, type TableSpec } from './DataTable';
import { Close } from './icons';
import { Legend, othersEntry } from './Legend';

const FIT_PADDING = 48;
const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)');
const MAX_FIT_SCALE = 2.5;
const DIRECTIONS: Record<string, [number, number]> = { ArrowRight: [1, 0], ArrowLeft: [-1, 0], ArrowDown: [0, 1], ArrowUp: [0, -1] };

export interface Link {
  node: number;
  count: number;
  weight: number;
  /** For directed graphs: this node is the source (it imports `node`). */
  outgoing: boolean;
}

/** What view-specific panels, tooltips and announcements can ask about the graph under the playhead. */
export interface GraphApi {
  data: GraphData;
  /** 0 or 1: which keyframe of the segment the playhead is closer to. */
  side: 0 | 1;
  pathOf: (i: number) => number;
  label: (i: number) => string;
  links: (i: number) => Link[];
  select: (id: number | null) => void;
}

interface GraphViewProps {
  segment: Accessor<GraphData | null>;
  t: Accessor<number>;
  radius: Radius;
  directed: boolean;
  roleDescription: string;
  ariaLabel: string;
  loadingText: string;
  emptyText: string;
  bar: JSX.Element;
  /** Show the data as a table instead of the chart. */
  table: boolean;
  tableOf: (g: GraphApi) => TableSpec;
  describe: (i: number, g: GraphApi) => string;
  tooltip: (i: number, g: GraphApi) => JSX.Element;
  panel: (i: number, g: GraphApi) => JSX.Element;
  /** Enter or double-click on a node; return true if handled (e.g. a folder was expanded). */
  onActivate?: (i: number, g: GraphApi) => boolean;
  /** Escape with nothing selected; return true if handled (e.g. a folder was collapsed). */
  onBack?: (focused: number, g: GraphApi) => boolean;
}

export function GraphView(props: GraphViewProps) {
  let host!: HTMLDivElement;
  let glCanvas!: HTMLCanvasElement;
  let overlay!: HTMLCanvasElement;
  let renderer: GraphRenderer | undefined;

  const [size, setSize] = createSignal({ w: 0, h: 0 });
  const [user, setUser] = createSignal<View>({ k: 1, x: 0, y: 0 });
  const [hover, setHover] = createSignal<{ i: number; x: number; y: number } | null>(null);
  const [selected, setSelected] = createSignal<number | null>(null);
  const [focusId, setFocusId] = createSignal<number | null>(null);
  const [glError, setGlError] = createSignal('');
  const [tick, setTick] = createSignal(0);
  const [spot, setSpot] = createSignal(-1);
  const [pinnedSpot, setPinnedSpot] = createSignal(false);

  const segment = props.segment;
  const t = props.t;

  createEffect(
    on(meta, () =>
      batch(() => {
        setSelected(null);
        setFocusId(null);
      }),
    ),
  );

  const indexOf = createMemo(() => new Map([...(segment()?.ids ?? [])].map((id, i) => [id, i])));
  const index = (id: number | null) => (id === null ? -1 : (indexOf().get(id) ?? -1));
  const side = (): 0 | 1 => (t() < 0.5 ? 0 : 1);
  const visible = (i: number) => (segment()?.size[i * 2 + side()] ?? 0) > 0;
  const pathOf = (i: number) => {
    const d = segment()!;
    return d.path[i * 2 + side()] || d.path[i * 2] || d.path[i * 2 + 1];
  };

  const links = (i: number): Link[] => {
    const d = segment();
    if (!d || i < 0) return [];
    const out: Link[] = [];
    for (let e = 0; e < d.ends.length / 2; e++) {
      const count = d.count[e * 2 + side()];
      if (!count) continue;
      const [a, b] = [d.ends[e * 2], d.ends[e * 2 + 1]];
      if (a === i || b === i) out.push({ node: a === i ? b : a, count, weight: d.weight[e * 2 + side()], outgoing: a === i });
    }
    return out.sort((x, y) => y.count - x.count || y.weight - x.weight);
  };

  /** File or folder name, prefixed with its parent when several visible nodes share it (package.json…). */
  const labels = createMemo(() => {
    const d = segment();
    pathsVersion();
    if (!d) return [];
    const at = (i: number) => d.path[i * 2 + 1] || d.path[i * 2];
    const counts = new Map<string, number>();
    for (let i = 0; i < d.ids.length; i++) counts.set(pathName(at(i)), (counts.get(pathName(at(i))) ?? 0) + 1);
    return [...d.ids.keys()].map((i) => {
      const name = pathName(at(i));
      const full = counts.get(name)! > 1 ? `${pathName(pathParent(at(i))) || '.'}/${name}` : name;
      return d.dir[i] ? `${full}/` : full;
    });
  });

  const api = (): GraphApi => ({ data: segment()!, side: side(), pathOf, label: (i) => labels()[i] ?? '', links, select: choose });

  const groupSlots = createMemo(() => new Map([...(meta()?.groupRank ?? [])].slice(0, OTHERS).map((g, slot) => [g, slot])));
  const slot = (group: number) => groupSlots().get(group) ?? OTHERS;
  const palette = createMemo(() => makePalette(theme(), document.documentElement));
  const legendEntries = createMemo(() => {
    pathsVersion();
    return [...groupSlots()].map(([g, slot]) => ({ label: g === 0 ? 'root files' : `${pathName(g)}/`, slot })).concat(othersEntry('Others'));
  });

  /** 1 selected, 0 normal, 2 dimmed: outside the selection's neighbourhood or the spotlit folder. */
  const nodeState = createMemo(() => {
    const d = segment();
    const sel = index(selected());
    const near = sel < 0 ? null : new Set(links(sel).map((l) => l.node));
    const lit = spot();
    return (i: number) => {
      if (i === sel) return 1;
      if (near && !near.has(i)) return 2;
      return lit >= 0 && d && slot(d.group[i]) !== lit ? 2 : 0;
    };
  });

  onMount(() => {
    try {
      renderer = new GraphRenderer(glCanvas);
    } catch (e) {
      console.error(e);
      setGlError(e instanceof Error ? e.message : String(e));
    }
    const ro = new ResizeObserver(([entry]) => setSize({ w: Math.round(entry.contentRect.width), h: Math.round(entry.contentRect.height) }));
    ro.observe(host);
    const z = zoom<HTMLCanvasElement, unknown>()
      .scaleExtent([0.25, 12])
      .on('zoom', (e) => setUser({ k: e.transform.k, x: e.transform.x, y: e.transform.y }));
    const surface = select(overlay).call(z).on('dblclick.zoom', null);
    resetZoom = () => surface.call(z.transform, zoomIdentity);
    onCleanup(() => {
      ro.disconnect();
      renderer?.dispose();
    });
  });
  let resetZoom = () => {};

  createEffect(() => renderer?.setPalette(palette()));
  createEffect(() => {
    const d = segment();
    groupSlots();
    const state = nodeState();
    if (d) renderer?.upload(d, slot, state, props.radius);
  });

  // The frame follows the graph as it grows, easing toward a fit instead of snapping.
  let base: View | null = null;
  const target = (): View | null => {
    const d = segment();
    const { w, h } = size();
    if (!d || !w || !h) return null;
    let [x0, y0, x1, y1] = [Infinity, Infinity, -Infinity, -Infinity];
    for (let i = 0; i < d.ids.length; i++) {
      for (const s of [0, 1]) {
        if (!d.size[i * 2 + s]) continue;
        const [x, y] = [d.pos[i * 4 + s * 2], d.pos[i * 4 + s * 2 + 1]];
        [x0, y0, x1, y1] = [Math.min(x0, x), Math.min(y0, y), Math.max(x1, x), Math.max(y1, y)];
      }
    }
    if (x0 > x1) return base;
    const k = Math.min((w - 2 * FIT_PADDING) / Math.max(x1 - x0, 1), (h - 2 * FIT_PADDING) / Math.max(y1 - y0, 1), MAX_FIT_SCALE);
    return { k, x: w / 2 - ((x0 + x1) / 2) * k, y: h / 2 - ((y0 + y1) / 2) * k };
  };
  const view = (): View => {
    const u = user();
    const b = base ?? { k: 1, x: 0, y: 0 };
    return { k: u.k * b.k, x: u.k * b.x + u.x, y: u.k * b.y + u.y };
  };

  let frame = 0;
  let last = performance.now();
  createEffect(() => {
    tick();
    const state = {
      d: segment(),
      t: t(),
      size: size(),
      user: user(),
      palette: palette(),
      hover: hover()?.i ?? -1,
      sel: index(selected()),
      focus: index(focusId()),
      states: nodeState(),
      labels: labels(),
    };
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame((now) => {
      const goal = target();
      if (goal) {
        const a = base && !reducedMotion.matches ? 1 - Math.exp(-(now - last) / 180) : 1;
        base = base ? { k: base.k + (goal.k - base.k) * a, x: base.x + (goal.x - base.x) * a, y: base.y + (goal.y - base.y) * a } : goal;
      }
      last = now;
      const { d, size: s } = state;
      const dpr = Math.min(devicePixelRatio, 2);
      renderer?.draw({ t: state.t, directed: props.directed, view: view(), width: s.w, height: s.h, dpr });
      if (d) {
        const pinned = [state.hover, state.sel, ...links(state.sel).map((l) => l.node)].filter((i) => i >= 0);
        drawGraphOverlay(overlay, d, {
          t: state.t,
          radius: props.radius,
          view: view(),
          width: s.w,
          height: s.h,
          dpr,
          palette: state.palette,
          name: (i) => state.labels[i] ?? '',
          pinned,
          dimmed: (i) => state.states(i) === 2,
          focus: state.focus,
        });
      }
      if (goal && base && Math.abs(goal.k - base.k) + Math.abs(goal.x - base.x) + Math.abs(goal.y - base.y) > 0.05) setTick((n) => n + 1);
    });
  });

  function choose(id: number | null) {
    batch(() => {
      setSelected(id);
      setFocusId(id);
    });
    const i = index(id);
    if (i >= 0) announce(props.describe(i, api()));
  }

  const focus = (i: number) => {
    setFocusId(segment()!.ids[i]);
    announce(props.describe(i, api()));
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.altKey || e.metaKey || e.ctrlKey) return;
    const d = segment();
    if (e.key === 'Escape') {
      if (selected() !== null) {
        e.preventDefault();
        return setSelected(null);
      }
      const f = index(focusId());
      if (d && props.onBack?.(f, api())) return e.preventDefault();
      if (focusId() !== null) {
        e.preventDefault();
        setFocusId(null);
      }
      return;
    }
    if (!d) return;
    const dir = DIRECTIONS[e.key];
    if (dir && !e.shiftKey) {
      e.preventDefault();
      const nodes = [...d.ids.keys()].filter(visible);
      if (!nodes.length) return;
      const current = index(focusId());
      if (current < 0) return focus(nodes.reduce((a, b) => (d.size[b * 2 + side()] > d.size[a * 2 + side()] ? b : a)));
      const from = nodeScreen(d, current, t(), view(), props.radius);
      let [best, bestScore] = [-1, Infinity];
      for (const i of nodes) {
        if (i === current) continue;
        const n = nodeScreen(d, i, t(), view(), props.radius);
        const [dx, dy] = [n.x - from.x, n.y - from.y];
        const along = dx * dir[0] + dy * dir[1];
        if (along <= 1) continue;
        const score = along + 2 * Math.abs(dx * dir[1] + dy * dir[0]);
        if (score < bestScore) [best, bestScore] = [i, score];
      }
      if (best >= 0) focus(best);
    } else if (e.key === 'Enter' && focusId() !== null) {
      e.preventDefault();
      const i = index(focusId());
      if (i >= 0 && props.onActivate?.(i, api())) return;
      choose(selected() === focusId() ? null : focusId());
    }
  };

  const pointerNode = (e: MouseEvent) => {
    const d = segment();
    if (!d) return -1;
    const r = overlay.getBoundingClientRect();
    return pickNode(d, t(), view(), props.radius, e.clientX - r.left, e.clientY - r.top);
  };

  const empty = () => {
    const d = segment();
    return !!d && ![...d.ids.keys()].some(visible);
  };
  const selectedIndex = () => index(selected());

  return (
    <div class="view-layout">
      <div class="view-bar">{props.bar}</div>
      <div
        ref={host}
        class="map"
        // biome-ignore lint/a11y/noNoninteractiveTabindex: a role="application" widget handles its own arrow-key navigation
        tabIndex={0}
        role="application"
        aria-busy={!segment()}
        aria-roledescription={props.roleDescription}
        aria-label={props.ariaLabel}
        onKeyDown={onKeyDown}
      >
        <canvas ref={glCanvas} class="layer" />
        <canvas
          ref={overlay}
          class="layer"
          onPointerMove={(e) => e.pointerType === 'mouse' && setHover({ i: pointerNode(e), x: e.offsetX, y: e.offsetY })}
          onPointerLeave={() => setHover(null)}
          onClick={(e) => {
            const i = pointerNode(e);
            choose(i < 0 || segment()!.ids[i] === selected() ? null : segment()!.ids[i]);
            host.focus({ preventScroll: true });
          }}
          onDblClick={(e) => {
            const i = pointerNode(e);
            if (i < 0) resetZoom();
            else props.onActivate?.(i, api());
          }}
        />
        <Show when={glError()}>
          <p class="view-empty">Your browser can’t draw this view (WebGL2 unavailable).</p>
        </Show>
        <Show when={!segment() && !glError() && commitCount()}>
          <p class="view-empty">{props.loadingText}</p>
        </Show>
        <Show when={empty()}>
          <p class="view-empty">{props.emptyText}</p>
        </Show>
        <Show when={hover() && hover()!.i >= 0 && segment() ? hover() : null}>
          {(h) => (
            <div class="cell-tip" style={{ '--x': `${Math.min(h().x + 14, size().w - 260)}px`, '--y': `${Math.min(h().y + 14, size().h - 70)}px` }}>
              {props.tooltip(h().i, api())}
            </div>
          )}
        </Show>
        <Show when={selectedIndex() >= 0}>{props.panel(selectedIndex(), api())}</Show>
        <Show when={props.table && segment()}>
          <DataTable spec={props.tableOf(api())} />
        </Show>
        <Legend
          title="Top-level folder"
          palette={palette()}
          entries={legendEntries()}
          spot={spot()}
          pinned={pinnedSpot()}
          onSpot={(s, pin) => {
            setSpot(s);
            if (pin !== undefined) setPinnedSpot(pin);
          }}
        />
      </div>
    </div>
  );
}

/** Closes a detail panel; the only way out on touch screens, where there's no Escape key. */
export const PanelClose = (props: { onClose: () => void }) => (
  <button type="button" class="btn ghost icon sheet-close" aria-label="Close details" onClick={props.onClose}>
    <Close />
  </button>
);
