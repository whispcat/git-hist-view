import { select } from 'd3-selection';
import { zoom, zoomIdentity } from 'd3-zoom';
import { batch, createEffect, createMemo, createSignal, For, type JSX, on, onCleanup, onMount, Show } from 'solid-js';
import { palette as makePalette, OTHERS } from '../render/palette';
import { type Camera, TreemapRenderer } from '../render/treemap';
import { type Colors, depthOf, drawOverlay, isDir, pair, pick, screenBox } from '../render/treemapOverlay';
import { findPath, fullPath, pathChain, pathName, pathParent } from '../state/paths';
import { commitCount, label, meta, requestTreemap } from '../state/session';
import { theme } from '../state/theme';
import { commitAt } from '../state/timeline';
import { COLOR_MODES, type ColorMode } from '../state/url';
import type { TreemapData } from '../worker/protocol';
import { announce } from './Announcer';
import { DataTable, type TableSpec } from './DataTable';
import { formatDate, formatNumber } from './format';
import { Close } from './icons';
import { Legend, othersEntry } from './Legend';
import { createSegments } from './segments';

const WINDOW_DAYS = 90;
const SLOTS = OTHERS;
const NO_OWNER = 0xffffffff;
export const MODE_LABELS: Record<ColorMode, string> = { churn: 'Churn', author: 'Author', language: 'Language' };

const DIRECTIONS: Record<string, [number, number]> = { ArrowRight: [1, 0], ArrowLeft: [-1, 0], ArrowDown: [0, 1], ArrowUp: [0, -1] };

export function TreemapView(props: {
  color: ColorMode;
  onColor: (c: ColorMode) => void;
  rootPath: string;
  onRootPath: (path: string) => void;
  controls: JSX.Element;
  table: boolean;
}) {
  let host!: HTMLDivElement;
  let glCanvas!: HTMLCanvasElement;
  let overlay!: HTMLCanvasElement;
  let renderer: TreemapRenderer | undefined;

  const [size, setSize] = createSignal({ w: 0, h: 0 });
  /** The folder shown, from the shareable path; resolves once the engine has sent that path's name. */
  const root = () => (props.rootPath ? (findPath(props.rootPath) ?? 0) : 0);
  const [camera, setCamera] = createSignal<Camera>({ k: 1, x: 0, y: 0 });
  const [hover, setHover] = createSignal<{ i: number; x: number; y: number } | null>(null);
  const [focusId, setFocusId] = createSignal<number | null>(null);
  const [spot, setSpot] = createSignal(-1);
  const [pinned, setPinned] = createSignal(false);
  const [glError, setGlError] = createSignal('');

  const { segment: shown, t } = createSegments<TreemapData>({
    family: () => (size().w && size().h ? `${root()}:${size().w}:${size().h}` : null),
    fetch: (k) => requestTreemap({ k, root: root(), width: size().w, height: size().h, windowDays: WINDOW_DAYS }),
  });

  createEffect(
    on(
      () => props.color,
      () =>
        batch(() => {
          setSpot(-1);
          setPinned(false);
        }),
    ),
  );

  createEffect(on(meta, () => setFocusId(null)));

  const authorSlots = createMemo(() => {
    const people = meta()?.people ?? [];
    const top = people
      .map((p, id) => [p.commits, id])
      .sort((a, b) => b[0] - a[0])
      .slice(0, SLOTS);
    return new Map(top.map(([, id], slot) => [id, slot]));
  });
  const langSlots = createMemo(() => new Map([...(meta()?.languageRank ?? [])].slice(0, SLOTS).map((lang, slot) => [lang, slot])));
  const authorSlot = (owner: number) => (owner === NO_OWNER ? OTHERS : (authorSlots().get(owner) ?? OTHERS));
  const langSlot = (lang: number) => langSlots().get(lang) ?? OTHERS;
  const palette = createMemo(() => makePalette(theme(), document.documentElement));
  const colors = (): Colors => ({ mode: props.color, palette: palette(), authorSlot, langSlot, spot: spot() });

  const indexOf = createMemo(() => new Map([...(shown()?.ids ?? [])].map((id, i) => [id, i])));
  const focusIndex = () => (focusId() === null ? -1 : (indexOf().get(focusId()!) ?? -1));

  onMount(() => {
    try {
      renderer = new TreemapRenderer(glCanvas);
    } catch (e) {
      console.error(e);
      setGlError(e instanceof Error ? e.message : String(e));
    }
    const ro = new ResizeObserver(([entry]) => setSize({ w: Math.round(entry.contentRect.width), h: Math.round(entry.contentRect.height) }));
    ro.observe(host);

    const z = zoom<HTMLCanvasElement, unknown>()
      .scaleExtent([1, 64])
      .on('zoom', (e) => setCamera({ k: e.transform.k, x: e.transform.x, y: e.transform.y }));
    const surface = select(overlay).call(z).on('dblclick.zoom', null);
    createEffect(() => {
      const { w, h } = size();
      z.extent([
        [0, 0],
        [w, h],
      ]).translateExtent([
        [0, 0],
        [w, h],
      ]);
    });
    createEffect(on([root, size], () => surface.call(z.transform, zoomIdentity)));
    onCleanup(() => {
      ro.disconnect();
      renderer?.dispose();
    });
  });

  createEffect(() => renderer?.setPalette(palette()));
  createEffect(() => {
    const d = shown();
    authorSlots();
    langSlots();
    if (d) renderer?.upload(d, authorSlot, langSlot);
  });

  let frame = 0;
  createEffect(() => {
    const state = { d: shown(), t: t(), camera: camera(), size: size(), colors: colors(), hover: hover()?.i ?? -1, focus: focusIndex() };
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      const { d, size: s } = state;
      const dpr = Math.min(devicePixelRatio, 2);
      renderer?.draw({ t: state.t, camera: state.camera, mode: props.color, spot: state.colors.spot, width: s.w, height: s.h, dpr });
      if (d)
        drawOverlay(overlay, d, {
          t: state.t,
          camera: state.camera,
          width: s.w,
          height: s.h,
          dpr,
          colors: state.colors,
          name: pathName,
          hover: state.hover,
          focus: state.focus,
        });
    });
  });

  const describe = (i: number) => {
    const d = shown()!;
    const id = d.ids[i];
    const loc = formatNumber(Math.round(pair(d.loc, i, t())));
    const churn = Math.round(pair(d.churn, i, t()));
    if (isDir(d, i)) return `${pathName(id)}, folder, ${loc} lines, ${formatNumber(churn)} changed in ${WINDOW_DAYS} days`;
    const owner = meta()!.people[d.owner[i * 2 + (t() < 0.5 ? 0 : 1)]]?.name;
    return `${pathName(id)}, ${loc} lines, ${formatNumber(churn)} changed in ${WINDOW_DAYS} days${owner ? `, mostly by ${owner}` : ''}`;
  };

  const focusCell = (id: number | null) => {
    setFocusId(id);
    const i = focusIndex();
    if (i >= 0) announce(describe(i));
  };

  const drill = (id: number) => {
    batch(() => {
      props.onRootPath(id ? fullPath(id) : '');
      setFocusId(null);
      setHover(null);
    });
    announce(`Opened ${fullPath(id) || 'repository root'}`);
  };

  /** Children of the current root, which keyboard focus moves between. */
  const topLevel = () => {
    const d = shown();
    if (!d) return [];
    const out: number[] = [];
    for (let i = 0; i < d.ids.length; i++) if (depthOf(d, i) === 0 && screenBox(d, i, t(), camera()).w > 0.5) out.push(i);
    return out;
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.altKey || e.metaKey || e.ctrlKey) return;
    // Going up must work even while the drilled-into folder's layout is still loading.
    if (e.key === 'Escape' && (root() !== 0 || focusId() !== null)) {
      e.preventDefault();
      if (root() === 0) return setFocusId(null);
      const from = root();
      drill(pathParent(from));
      return setFocusId(from);
    }
    const d = shown();
    if (!d) return;
    const cells = topLevel();
    const current = focusIndex();
    const dir = DIRECTIONS[e.key];
    if (dir && !e.shiftKey) {
      e.preventDefault();
      if (!cells.length) return;
      if (current < 0) return focusCell(d.ids[cells[0]]);
      const from = screenBox(d, current, t(), camera());
      const [cx, cy] = [from.x + from.w / 2, from.y + from.h / 2];
      let best = -1;
      let bestScore = Infinity;
      for (const i of cells) {
        if (i === current) continue;
        const b = screenBox(d, i, t(), camera());
        const [dx, dy] = [b.x + b.w / 2 - cx, b.y + b.h / 2 - cy];
        const along = dx * dir[0] + dy * dir[1];
        if (along <= 1) continue;
        const score = along + 2 * Math.abs(dx * dir[1] + dy * dir[0]);
        if (score < bestScore) [best, bestScore] = [i, score];
      }
      if (best >= 0) focusCell(d.ids[best]);
    } else if (e.key === 'Enter' && current >= 0) {
      e.preventDefault();
      if (isDir(d, current)) drill(d.ids[current]);
    }
  };

  const pointerCell = (e: MouseEvent) => {
    const d = shown();
    if (!d) return -1;
    const r = overlay.getBoundingClientRect();
    return pick(d, t(), camera(), e.clientX - r.left, e.clientY - r.top);
  };

  const legendEntries = createMemo(() => {
    const m = meta();
    if (!m) return [];
    if (props.color === 'author') {
      return [...authorSlots()].map(([id, slot]) => ({ label: m.people[id].name, slot })).concat(othersEntry('Others'));
    }
    return [...langSlots()].map(([lang, slot]) => ({ label: m.languages[lang], slot })).concat(othersEntry('Other'));
  });

  const tableOf = (d: TreemapData): TableSpec => {
    const side = t() < 0.5 ? 0 : 1;
    const rows: (string | number)[][] = [];
    for (let i = 0; i < d.ids.length; i++) {
      const loc = d.loc[i * 2 + side];
      if (isDir(d, i) || !loc) continue;
      rows.push([fullPath(d.ids[i]), loc, d.churn[i * 2 + side], meta()!.people[d.owner[i * 2 + side]]?.name ?? '', meta()!.languages[d.lang[i]] ?? '']);
    }
    rows.sort((a, b) => (b[1] as number) - (a[1] as number));
    const where = fullPath(root()) || label();
    return {
      caption: `Files in ${where} at ${formatDate(meta()!.times[commitAt()])}, largest first`,
      columns: [
        { label: 'File' },
        { label: 'Lines', numeric: true },
        { label: `Changed (${WINDOW_DAYS} days)`, numeric: true },
        { label: 'Owns most lines' },
        { label: 'Language' },
      ],
      rows,
    };
  };

  const details = () => {
    const d = shown();
    const h = hover();
    const i = h?.i ?? focusIndex();
    if (!d || i < 0 || i >= d.ids.length) return null;
    const box = screenBox(d, i, t(), camera());
    const at = h ? { x: h.x, y: h.y } : { x: box.x + 8, y: box.y + box.h };
    const pinned = !h;
    const owner = meta()!.people[d.owner[i * 2 + (t() < 0.5 ? 0 : 1)]]?.name;
    return {
      at,
      pinned,
      path: fullPath(d.ids[i]),
      dir: isDir(d, i),
      loc: Math.round(pair(d.loc, i, t())),
      churn: Math.round(pair(d.churn, i, t())),
      owner: isDir(d, i) ? undefined : owner,
      lang: isDir(d, i) ? undefined : meta()!.languages[d.lang[i]],
    };
  };

  return (
    <div class="view-layout">
      <div class="view-bar">
        <nav class="crumbs" aria-label="Folder">
          <button type="button" class="crumb" onClick={() => drill(0)} aria-current={root() === 0 ? 'page' : undefined}>
            {label()}
          </button>
          <For each={pathChain(root())}>
            {(id) => (
              <>
                <span class="crumb-sep" aria-hidden="true">
                  /
                </span>
                <button type="button" class="crumb" onClick={() => drill(id)} aria-current={root() === id ? 'page' : undefined}>
                  {pathName(id)}
                </button>
              </>
            )}
          </For>
        </nav>
        {props.controls}
        <fieldset class="modes">
          <legend class="sr-only">Color by</legend>
          <For each={COLOR_MODES}>
            {(mode) => (
              <button type="button" class="mode" aria-pressed={props.color === mode} onClick={() => props.onColor(mode)}>
                {MODE_LABELS[mode]}
              </button>
            )}
          </For>
        </fieldset>
      </div>
      <div
        ref={host}
        class="map"
        // biome-ignore lint/a11y/noNoninteractiveTabindex: a role="application" widget handles its own arrow-key navigation
        tabIndex={0}
        role="application"
        aria-busy={!shown()}
        aria-roledescription="treemap"
        aria-label={`Files of ${fullPath(root()) || 'the repository'} at ${meta() ? formatDate(meta()!.times[commitAt()]) : ''}. Arrow keys move between items, Enter opens a folder, Escape goes up, C changes colors.`}
        onKeyDown={onKeyDown}
      >
        <canvas ref={glCanvas} class="layer" />
        <canvas
          ref={overlay}
          class="layer"
          onPointerMove={(e) => e.pointerType === 'mouse' && setHover({ i: pointerCell(e), x: e.offsetX, y: e.offsetY })}
          onPointerLeave={() => setHover(null)}
          onClick={(e) => {
            const i = pointerCell(e);
            const d = shown();
            if (!d || i < 0) return focusCell(null);
            focusCell(d.ids[i]);
            host.focus({ preventScroll: true });
          }}
          onDblClick={(e) => {
            const i = pointerCell(e);
            const d = shown();
            if (d && i >= 0 && isDir(d, i)) drill(d.ids[i]);
          }}
        />
        <Show when={glError()}>
          <p class="view-empty">Your browser can’t draw this view (WebGL2 unavailable).</p>
        </Show>
        <Show when={!shown() && !glError() && commitCount()}>
          <p class="view-empty">Laying out files…</p>
        </Show>
        <Show when={details()}>
          {(info) => (
            <div
              class="cell-tip"
              classList={{ pinned: info().pinned }}
              style={{ '--x': `${Math.min(info().at.x + 14, size().w - 260)}px`, '--y': `${Math.min(info().at.y + 14, size().h - 90)}px` }}
            >
              <Show when={info().pinned}>
                <button type="button" class="btn ghost icon sheet-close" aria-label="Close details" onClick={() => focusCell(null)}>
                  <Close />
                </button>
              </Show>
              <div class="tip-path mono">{info().path}</div>
              <div class="tip-meta">
                {info().dir ? 'Folder · ' : ''}
                {formatNumber(info().loc)} lines · {formatNumber(info().churn)} changed in {WINDOW_DAYS} days
              </div>
              <Show when={info().owner || info().lang}>
                <div class="tip-meta">{[info().owner && `Mostly ${info().owner}`, info().lang].filter(Boolean).join(' · ')}</div>
              </Show>
            </div>
          )}
        </Show>
        <Show when={props.table && shown()}>{(d) => <DataTable spec={tableOf(d())} />}</Show>
        <Legend
          title={props.color === 'churn' ? `Lines changed, last ${WINDOW_DAYS} days` : props.color === 'author' ? 'Owns most lines' : 'Language'}
          palette={palette()}
          entries={props.color === 'churn' ? undefined : legendEntries()}
          spot={spot()}
          pinned={pinned()}
          onSpot={(slot, pin) => {
            setSpot(slot);
            if (pin !== undefined) setPinned(pin);
          }}
        />
      </div>
    </div>
  );
}
