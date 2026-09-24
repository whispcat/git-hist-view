import type { ColorMode } from '../state/url';
import type { TreemapData } from '../worker/protocol';
import { churnLevel, css, inkOn, type Palette, RAMP_SIZE, type Rgb } from './palette';
import type { Camera } from './treemap';

interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

const LABEL_BUDGET = 600;
const FONT = '500 11px "Geist Variable", system-ui, sans-serif';
const SMALL = '400 10px "Geist Mono Variable", ui-monospace, monospace';

export const isDir = (d: TreemapData, i: number) => (d.flags[i] & 1) === 1;
const hasHeader = (d: TreemapData, i: number) => (d.flags[i] & 2) === 2;
export const depthOf = (d: TreemapData, i: number) => d.flags[i] >> 2;
export const pair = (a: ArrayLike<number>, i: number, t: number) => a[i * 2] + (a[i * 2 + 1] - a[i * 2]) * t;

/** A cell's rectangle in screen pixels at tween position `t`. */
export function screenBox(d: TreemapData, i: number, t: number, c: Camera): Box {
  const g = d.geom;
  const o = i * 8;
  const lerp = (k: number) => g[o + k] + (g[o + 4 + k] - g[o + k]) * t;
  return { x: lerp(0) * c.k + c.x, y: lerp(1) * c.k + c.y, w: lerp(2) * c.k, h: lerp(3) * c.k };
}

/** Deepest cell under a screen point; cells are ordered parents first, so scan backwards. */
export function pick(d: TreemapData, t: number, c: Camera, px: number, py: number) {
  for (let i = d.ids.length - 1; i >= 0; i--) {
    const b = screenBox(d, i, t, c);
    if (b.w > 0.5 && b.h > 0.5 && px >= b.x && px < b.x + b.w && py >= b.y && py < b.y + b.h) return i;
  }
  return -1;
}

export interface Colors {
  mode: ColorMode;
  palette: Palette;
  authorSlot: (owner: number) => number;
  langSlot: (lang: number) => number;
  spot: number;
}

/** CPU mirror of the shader's file color, for label contrast and legends. */
function fillOf(d: TreemapData, i: number, t: number, c: Colors): Rgb {
  const p = c.palette;
  if (c.mode === 'churn') {
    const lines = pair(d.churn, i, t);
    if (lines < 0.5) return p.zero;
    const k = Math.round(churnLevel(lines) * (RAMP_SIZE - 1)) * 4;
    return [p.ramp[k], p.ramp[k + 1], p.ramp[k + 2]];
  }
  const slot = c.mode === 'author' ? c.authorSlot(d.owner[i * 2 + (t < 0.5 ? 0 : 1)]) : c.langSlot(d.lang[i]);
  const rgb = p.categorical[slot];
  if (c.spot < 0 || slot === c.spot) return rgb;
  return rgb.map((v, k) => v + (p.zero[k] - v) * 0.8) as Rgb;
}

const widths = new Map<string, number>();

function fit(ctx: CanvasRenderingContext2D, text: string, max: number) {
  const width = (s: string) => {
    const key = ctx.font + s;
    let w = widths.get(key);
    if (w === undefined) {
      w = ctx.measureText(s).width;
      widths.set(key, w);
    }
    return w;
  };
  if (width(text) <= max) return text;
  let [lo, hi] = [0, text.length];
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (width(`${text.slice(0, mid)}…`) <= max) lo = mid;
    else hi = mid - 1;
  }
  return lo > 1 ? `${text.slice(0, lo)}…` : '';
}

interface OverlayState {
  t: number;
  camera: Camera;
  width: number;
  height: number;
  dpr: number;
  colors: Colors;
  name: (id: number) => string;
  hover: number;
  focus: number;
}

export function drawOverlay(canvas: HTMLCanvasElement, d: TreemapData, s: OverlayState) {
  const [w, h] = [Math.round(s.width * s.dpr), Math.round(s.height * s.dpr)];
  if (canvas.width !== w || canvas.height !== h) [canvas.width, canvas.height] = [w, h];
  const ctx = canvas.getContext('2d')!;
  ctx.setTransform(s.dpr, 0, 0, s.dpr, 0, 0);
  ctx.clearRect(0, 0, s.width, s.height);
  ctx.textBaseline = 'alphabetic';
  const p = s.colors.palette;
  let budget = LABEL_BUDGET;

  for (let i = 0; i < d.ids.length && budget > 0; i++) {
    const b = screenBox(d, i, s.t, s.camera);
    if (b.x > s.width || b.y > s.height || b.x + b.w < 0 || b.y + b.h < 0) continue;
    if (isDir(d, i)) {
      if (!hasHeader(d, i) || b.w < 48) continue;
      ctx.font = FONT;
      ctx.fillStyle = css(p.ink2);
      const text = fit(ctx, s.name(d.ids[i]), b.w - 12);
      if (text) ctx.fillText(text, b.x + 6, b.y + 12);
    } else {
      if (b.w < 36 || b.h < 16) continue;
      const ink = inkOn(fillOf(d, i, s.t, s.colors));
      ctx.font = FONT;
      ctx.fillStyle = css(ink);
      const text = fit(ctx, s.name(d.ids[i]), b.w - 10);
      if (!text) continue;
      ctx.fillText(text, b.x + 5, b.y + 13);
      if (b.h >= 32 && b.w >= 44) {
        ctx.font = SMALL;
        ctx.globalAlpha = 0.72;
        ctx.fillText(Math.round(pair(d.loc, i, s.t)).toLocaleString(), b.x + 5, b.y + 26);
        ctx.globalAlpha = 1;
      }
    }
    budget--;
  }

  const outline = (i: number, color: Rgb, width: number) => {
    if (i < 0 || i >= d.ids.length) return;
    const b = screenBox(d, i, s.t, s.camera);
    ctx.lineWidth = width;
    ctx.strokeStyle = css(p.surface);
    ctx.strokeRect(b.x - width, b.y - width, b.w + width * 2, b.h + width * 2);
    ctx.strokeStyle = css(color);
    ctx.strokeRect(b.x + width / 2, b.y + width / 2, b.w - width, b.h - width);
  };
  outline(s.hover, p.ink, 1.5);
  outline(s.focus, p.accent, 2.5);
}
