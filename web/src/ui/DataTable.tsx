import { For } from 'solid-js';
import { formatNumber } from './format';

const LIMIT = 500;

export interface TableSpec {
  caption: string;
  columns: { label: string; numeric?: boolean }[];
  rows: (string | number)[][];
}

/** The same data as the chart as a plain table, for screen readers, keyboard users and copying. */
export function DataTable(props: { spec: TableSpec }) {
  const shown = () => props.spec.rows.slice(0, LIMIT);
  return (
    // biome-ignore lint/a11y/noNoninteractiveTabindex: scrollable regions must be focusable so keyboards can scroll them
    <section class="table-view" tabIndex={0} aria-label={props.spec.caption}>
      <table>
        <caption>
          {props.spec.caption}
          {props.spec.rows.length > LIMIT ? ` · first ${formatNumber(LIMIT)} of ${formatNumber(props.spec.rows.length)}` : ''}
        </caption>
        <thead>
          <tr>
            <For each={props.spec.columns}>
              {(c) => (
                <th scope="col" classList={{ numeric: c.numeric }}>
                  {c.label}
                </th>
              )}
            </For>
          </tr>
        </thead>
        <tbody>
          <For each={shown()}>
            {(row) => (
              <tr>
                <For each={row}>
                  {(cell, i) => (
                    <td classList={{ numeric: props.spec.columns[i()]?.numeric, mono: i() === 0 }}>{typeof cell === 'number' ? formatNumber(cell) : cell}</td>
                  )}
                </For>
              </tr>
            )}
          </For>
        </tbody>
      </table>
    </section>
  );
}
