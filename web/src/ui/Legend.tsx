import { For, Show } from 'solid-js';
import { CHURN_MAX, css, OTHERS, type Palette } from '../render/palette';

interface LegendEntry {
  label: string;
  slot: number;
}

const TICKS = [1, 10, 100, CHURN_MAX];

/** A ramp legend when `entries` is omitted; otherwise a list whose items spotlight their slot on hover or click. */
export function Legend(props: {
  title: string;
  palette: Palette;
  entries?: LegendEntry[];
  spot: number;
  pinned: boolean;
  onSpot: (slot: number, pin?: boolean) => void;
}) {
  const ramp = () => {
    const stops = [0, 0.25, 0.5, 0.75, 1].map((f) => {
      const k = Math.round(f * 255) * 4;
      return `rgb(${props.palette.ramp[k]} ${props.palette.ramp[k + 1]} ${props.palette.ramp[k + 2]}) ${f * 100}%`;
    });
    return `linear-gradient(to right, ${stops.join(', ')})`;
  };
  return (
    <details class="legend" open={!matchMedia('(max-width: 720px)').matches}>
      <summary class="legend-title">{props.title}</summary>
      <Show
        when={!props.entries}
        fallback={
          <ul class="legend-list">
            <For each={props.entries}>
              {(e) => (
                <li>
                  <button
                    type="button"
                    class="legend-item"
                    aria-pressed={props.pinned && props.spot === e.slot}
                    onMouseEnter={() => !props.pinned && props.onSpot(e.slot)}
                    onMouseLeave={() => !props.pinned && props.onSpot(-1)}
                    onFocus={() => !props.pinned && props.onSpot(e.slot)}
                    onBlur={() => !props.pinned && props.onSpot(-1)}
                    onClick={() => props.onSpot(props.pinned && props.spot === e.slot ? -1 : e.slot, !(props.pinned && props.spot === e.slot))}
                  >
                    <span class="swatch" style={{ background: css(props.palette.categorical[e.slot]) }} />
                    <span class="legend-label">{e.label}</span>
                  </button>
                </li>
              )}
            </For>
          </ul>
        }
      >
        <div class="ramp-row">
          <span class="swatch" style={{ background: css(props.palette.zero) }} title="No changes" />
          <div class="ramp" style={{ background: ramp() }} />
        </div>
        <div class="ramp-ticks">
          <span>0</span>
          <For each={TICKS}>
            {(t, i) => <span style={{ left: `${(Math.log1p(t) / Math.log1p(CHURN_MAX)) * 100}%` }}>{i() === TICKS.length - 1 ? '1k+' : t}</span>}
          </For>
        </div>
      </Show>
    </details>
  );
}

export const othersEntry = (label: string): LegendEntry => ({ label, slot: OTHERS });
