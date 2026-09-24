import { rgb } from 'd3-color';
import { interpolateLab, piecewise } from 'd3-interpolate';
import type { Theme } from '../state/theme';

export type Rgb = [number, number, number];

/** Categorical slots (reference palette with magenta dropped, since pink is our accent); slot 7 is "others". */
const CATEGORICAL: Record<Theme, string[]> = {
  light: ['#2a78d6', '#eb6834', '#1baf7a', '#eda100', '#008300', '#4a3aa7', '#e34948', '#cfcec7'],
  dark: ['#3987e5', '#d95926', '#199e70', '#c98500', '#008300', '#9085e9', '#e66767', '#4a4944'],
};

/** Sequential blue, anchored at the surface end: light-to-dark on light, dark-to-light on dark. */
const RAMP: Record<Theme, string[]> = {
  light: ['#cde2fb', '#86b6ef', '#3987e5', '#256abf', '#184f95', '#0d366b'],
  dark: ['#184f95', '#1c5cab', '#2a78d6', '#5598e7', '#86b6ef', '#cde2fb'],
};

export const OTHERS = 7;
export const CHURN_MAX = 1000;
export const RAMP_SIZE = 256;

export interface Palette {
  categorical: Rgb[];
  ramp: Uint8Array;
  /** Files with no recent change in churn mode. */
  zero: Rgb;
  dir: Rgb;
  ink: Rgb;
  ink2: Rgb;
  accent: Rgb;
  surface: Rgb;
}

const toRgb = (color: string): Rgb => {
  const c = rgb(color);
  return [c.r, c.g, c.b];
};

export function palette(theme: Theme, root: Element): Palette {
  const css = getComputedStyle(root);
  const token = (name: string) => toRgb(css.getPropertyValue(name).trim());
  const interpolate = piecewise(interpolateLab, RAMP[theme]);
  const ramp = new Uint8Array(RAMP_SIZE * 4);
  for (let i = 0; i < RAMP_SIZE; i++) {
    ramp.set([...toRgb(interpolate(i / (RAMP_SIZE - 1))), 255], i * 4);
  }
  return {
    categorical: CATEGORICAL[theme].map(toRgb),
    ramp,
    zero: theme === 'light' ? [233, 232, 227] : [42, 42, 40],
    dir: token('--bg-sunken'),
    ink: token('--ink'),
    ink2: token('--ink-2'),
    accent: token('--accent'),
    surface: token('--bg'),
  };
}

/** Position of a churn value on the log-scaled ramp, 0..1. */
export const churnLevel = (lines: number) => Math.min(Math.log1p(lines) / Math.log1p(CHURN_MAX), 1);

export const css = ([r, g, b]: Rgb) => `rgb(${r} ${g} ${b})`;

/** Picks dark or light label ink for a fill by relative luminance. */
export function inkOn([r, g, b]: Rgb): Rgb {
  const lin = (byte: number) => {
    const c = byte / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  const luminance = 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
  return luminance > 0.36 ? [11, 11, 11] : [255, 255, 255];
}
