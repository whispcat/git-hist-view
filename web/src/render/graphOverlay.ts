import type { GraphData } from '../worker/protocol';
import type { Radius, View } from './graph';
import { css, type Palette } from './palette';

const FONT = '500 11px "Geist Variable", system-ui, sans-serif';
const HALO = 3;

export function nodeScreen(d: GraphData, i: number, t: number, v: View, radius: Radius) {
  const p = d.pos;
  const x = p[i * 4] + (p[i * 4 + 2] - p[i * 4]) * t;
  const y = p[i * 4 + 1] + (p[i * 4 + 3] - p[i * 4 + 1]) * t;
  const [ra, rb] = [radius(d.size[i * 2]), radius(d.size[i * 2 + 1])];
  const r = ra + (rb - ra) * t;
  return { x: x * v.k + v.x, y: y * v.k + v.y, r };
}

/** Nearest visible node within a few pixels of its rim. */
export function pickNode(d: GraphData, t: number, v: View, radius: Radius, px: number, py: number) {
  let best = -1;
  let bestGap = 6;
  for (let i = 0; i < d.ids.length; i++) {
    const n = nodeScreen(d, i, t, v, radius);
    if (n.r < 0.5) continue;
    const gap = Math.hypot(n.x - px, n.y - py) - n.r;
    if (gap < bestGap) [best, bestGap] = [i, gap];
  }
  return best;
}

interface LabelState {
  t: number;
  radius: Radius;
  view: View;
  width: number;
  height: number;
  dpr: number;
  palette: Palette;
  name: (i: number) => string;
  /** Always labelled, in priority order (hovered, selected, its neighbours). */
  pinned: number[];
  dimmed: (i: number) => boolean;
  focus: number;
}

/** Labels pinned nodes first, then the largest nodes wherever a label still fits without overlapping. */
export function drawGraphOverlay(canvas: HTMLCanvasElement, d: GraphData, s: LabelState) {
  const [w, h] = [Math.round(s.width * s.dpr), Math.round(s.height * s.dpr)];
  if (canvas.width !== w || canvas.height !== h) [canvas.width, canvas.height] = [w, h];
  const ctx = canvas.getContext('2d')!;
  ctx.setTransform(s.dpr, 0, 0, s.dpr, 0, 0);
  ctx.clearRect(0, 0, s.width, s.height);
  ctx.font = FONT;
  ctx.textBaseline = 'middle';
  ctx.lineJoin = 'round';

  const placed: [number, number, number, number][] = [];
  const pinned = new Set(s.pinned);
  const bySize = [...d.ids.keys()]
    .filter((i) => !pinned.has(i))
    .sort((a, b) => nodeScreen(d, b, s.t, s.view, s.radius).r - nodeScreen(d, a, s.t, s.view, s.radius).r);
  for (const i of [...s.pinned, ...bySize]) {
    const n = nodeScreen(d, i, s.t, s.view, s.radius);
    if (n.r < 0.5 || (!pinned.has(i) && (n.r < 5 || s.dimmed(i)))) continue;
    const text = s.name(i);
    const width = ctx.measureText(text).width;
    const box: [number, number, number, number] = [n.x + n.r + 4, n.y - 8, width + 2 * HALO, 16];
    if (box[0] + box[2] > s.width || box[1] < 0 || box[1] + box[3] > s.height) continue;
    const overlaps = placed.some(([x, y, bw, bh]) => box[0] < x + bw && x < box[0] + box[2] && box[1] < y + bh && y < box[1] + box[3]);
    if (overlaps && !pinned.has(i)) continue;
    placed.push(box);
    ctx.globalAlpha = s.dimmed(i) ? 0.45 : 1;
    ctx.lineWidth = HALO * 2;
    ctx.strokeStyle = css(s.palette.surface);
    ctx.strokeText(text, box[0] + HALO, n.y);
    ctx.fillStyle = css(pinned.has(i) ? s.palette.ink : s.palette.ink2);
    ctx.fillText(text, box[0] + HALO, n.y);
  }
  ctx.globalAlpha = 1;

  if (s.focus >= 0 && s.focus < d.ids.length) {
    const n = nodeScreen(d, s.focus, s.t, s.view, s.radius);
    ctx.lineWidth = 2.5;
    ctx.strokeStyle = css(s.palette.accent);
    ctx.beginPath();
    ctx.arc(n.x, n.y, n.r + 4, 0, Math.PI * 2);
    ctx.stroke();
  }
}
