import { timeDay, timeMonth, timeWeek, timeYear } from 'd3-time';
import { createEffect, createMemo, createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { commitChurn, commitCount, frontier, meta } from '../state/session';
import { theme } from '../state/theme';
import { commitAt, commitOfPos, frameCount, pause, playing, pos, posOfCommit, seek, settle, stepBy, togglePlay } from '../state/timeline';
import { formatDate, formatNumber, shortSha } from './format';
import { Pause, Play } from './icons';

const MIN_TICK_GAP = 52;
const DAY = 86_400_000;

function lowerBound(times: Float64Array, t: number) {
  let lo = 0;
  let hi = times.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (times[mid] < t) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/** Date ticks placed by commit index (the track is linear in commits, not time), thinned so labels never collide. */
function ticks(times: Float64Array, width: number) {
  const n = times.length;
  if (n < 2 || width <= 0) return [];
  const [t0, t1] = [times[0] * 1000, times[n - 1] * 1000];
  const span = t1 - t0;
  const [interval, options]: [typeof timeYear, Intl.DateTimeFormatOptions] =
    span > 3 * 365 * DAY
      ? [timeYear, { year: 'numeric' }]
      : span > 120 * DAY
        ? [timeMonth, { month: 'short', year: '2-digit' }]
        : span > 14 * DAY
          ? [timeWeek, { month: 'short', day: 'numeric' }]
          : [timeDay, { month: 'short', day: 'numeric' }];
  const format = new Intl.DateTimeFormat(undefined, options);
  const out: { x: number; label: string }[] = [];
  let last = -Infinity;
  for (const d of interval.range(new Date(t0), new Date(t1))) {
    const x = (lowerBound(times, d.getTime() / 1000) / (n - 1)) * width;
    if (x - last < MIN_TICK_GAP || x < 14 || x > width - 14) continue;
    out.push({ x: x / width, label: format.format(d) });
    last = x;
  }
  return out;
}

function draw(canvas: HTMLCanvasElement, churn: Uint32Array, n: number, analyzed: number, played: number) {
  const dpr = Math.min(devicePixelRatio, 2);
  const [w, h] = [canvas.clientWidth, canvas.clientHeight];
  if (!w || !h) return;
  if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
  }
  const ctx = canvas.getContext('2d')!;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  const css = getComputedStyle(canvas);
  const color = (name: string) => css.getPropertyValue(name).trim();

  const bins = Math.max(1, Math.min(n, Math.floor(w / 3)));
  const sums = new Float64Array(bins);
  for (let c = 0; c < analyzed; c++) sums[Math.floor((c * bins) / n)] += churn[c];
  // A few giant commits (vendoring, reformatting) would flatten everything; cap at the 98th percentile.
  const sorted = sums.filter((v) => v > 0).sort();
  const cap = sorted[Math.floor(sorted.length * 0.98)] || 1;
  const binW = w / bins;
  const [bar, past, pending] = [color('--viz-bar'), color('--ink-3'), color('--viz-bar-pending')];
  for (let i = 0; i < bins; i++) {
    const x = i * binW;
    if (((i + 1) * n) / bins > analyzed) {
      ctx.fillStyle = pending;
      ctx.fillRect(x, h - 1, Math.max(1, binW - 1), 1);
    } else if (sums[i] > 0) {
      const bh = Math.max(1.5, Math.sqrt(Math.min(sums[i], cap) / cap) * (h - 4));
      ctx.fillStyle = ((i + 0.5) * n) / bins <= played ? past : bar;
      ctx.fillRect(x, h - bh, Math.max(1, binW - 1), bh);
    }
  }
}

export function Scrubber() {
  let track!: HTMLDivElement;
  let canvas!: HTMLCanvasElement;
  const [width, setWidth] = createSignal(0);
  const [hover, setHover] = createSignal<number | null>(null);
  /** A press becomes a drag once it moves a few pixels; a plain click glides to the nearest keyframe instead. */
  let press: { x: number; dragging: boolean } | null = null;

  const n = commitCount;
  const current = () => commitAt();
  const x = () => (n() > 1 ? commitOfPos(pos()) / (n() - 1) : 0);
  const tickList = createMemo(() => (meta() ? ticks(meta()!.times, width()) : []));

  onMount(() => {
    const ro = new ResizeObserver(() => setWidth(track.clientWidth));
    ro.observe(track);
    onCleanup(() => ro.disconnect());
  });

  createEffect(() => {
    width();
    theme();
    if (meta()) draw(canvas, commitChurn(), n(), frontier(), commitOfPos(pos()));
  });

  const commitFromEvent = (e: PointerEvent) => {
    const r = track.getBoundingClientRect();
    return Math.min(Math.max((e.clientX - r.left) / r.width, 0), 1) * (n() - 1);
  };

  const describe = (c: number) => {
    const m = meta()!;
    return `${formatDate(m.times[c])}, commit ${shortSha(m.oids, c)}: ${m.subjects[c]}`;
  };

  return (
    <section class="scrubber" aria-label="Timeline">
      <button type="button" class="btn icon play" onClick={togglePlay} aria-label={playing() ? 'Pause' : 'Play history'} title="Play / pause (Space)">
        <Show when={playing()} fallback={<Play />}>
          <Pause />
        </Show>
      </button>
      <div
        ref={track}
        class="track"
        onPointerDown={(e) => {
          press = { x: e.clientX, dragging: false };
          track.setPointerCapture(e.pointerId);
          pause();
          setHover(null);
        }}
        onPointerMove={(e) => {
          if (!press) {
            if (e.pointerType === 'mouse') setHover(Math.round(commitFromEvent(e)));
            return;
          }
          if (!press.dragging && Math.abs(e.clientX - press.x) < 4) return;
          press.dragging = true;
          seek(posOfCommit(commitFromEvent(e)));
        }}
        onPointerUp={(e) => {
          if (!press) return;
          const to = press.dragging ? pos() : posOfCommit(commitFromEvent(e));
          press = null;
          settle(to);
        }}
        onPointerCancel={() => {
          press = null;
          settle(pos());
        }}
        onPointerLeave={() => setHover(null)}
      >
        <canvas ref={canvas} />
        <div class="ticks" aria-hidden="true">
          <For each={tickList()}>{(t) => <span style={{ left: `${t.x * 100}%` }}>{t.label}</span>}</For>
        </div>
        <div
          class="playhead"
          role="slider"
          tabIndex={0}
          aria-label="Position in history"
          aria-valuemin={0}
          aria-valuemax={Math.max(0, frameCount() - 1)}
          aria-valuenow={Math.round(pos())}
          aria-valuetext={meta() ? describe(current()) : undefined}
          style={{ left: `${x() * 100}%` }}
          onKeyDown={(e) => {
            const delta = { ArrowUp: 1, ArrowRight: 1, ArrowDown: -1, ArrowLeft: -1, PageUp: 10, PageDown: -10 }[e.key];
            if (delta === undefined) return;
            e.preventDefault();
            stepBy(e.shiftKey ? delta * 10 : delta);
          }}
        />
        <Show when={hover() !== null && meta()}>
          <HoverTip commit={hover()!} left={(hover()! / Math.max(1, n() - 1)) * 100} />
        </Show>
      </div>
      <Show when={meta()}>
        <CommitInfo commit={current()} />
      </Show>
    </section>
  );
}

function HoverTip(props: { commit: number; left: number }) {
  const m = meta()!;
  const analyzed = () => props.commit < frontier();
  return (
    <div class="hover-tip" style={{ left: `clamp(140px, ${props.left}%, calc(100% - 140px))` }}>
      <div class="subject">{m.subjects[props.commit]}</div>
      <div class="meta">
        {formatDate(m.times[props.commit])} · <span class="mono">{shortSha(m.oids, props.commit)}</span> · {m.people[m.authors[props.commit]].name}
        {analyzed() ? ` · ${formatNumber(commitChurn()[props.commit])} lines` : ''}
      </div>
    </div>
  );
}

function CommitInfo(props: { commit: number }) {
  const m = () => meta()!;
  return (
    <div class="commit">
      <div class="meta">
        <span class="date">{formatDate(m().times[props.commit])}</span> · <span class="mono">{shortSha(m().oids, props.commit)}</span> ·{' '}
        {m().people[m().authors[props.commit]].name}
      </div>
      <div class="subject" title={m().subjects[props.commit]}>
        {m().subjects[props.commit]}
      </div>
    </div>
  );
}
