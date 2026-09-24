import type { ParseRequest } from './parse.worker';

const SIZE = Math.max(1, Math.min(4, (navigator.hardwareConcurrency || 2) - 1));

/** Tree-sitter import extraction spread over a few workers, created on first use. */
export class ParsePool {
  private workers: Worker[] = [];
  private pending = new Map<number, (records: string[]) => void>();
  private nextId = 0;

  private start() {
    for (let i = 0; i < SIZE; i++) {
      const w = new Worker(new URL('./parse.worker.ts', import.meta.url), { type: 'module' });
      w.onmessage = ({ data }: MessageEvent<{ id: number; records: string[] }>) => {
        this.pending.get(data.id)?.(data.records);
        this.pending.delete(data.id);
      };
      this.workers.push(w);
    }
  }

  /** Splits the batch by bytes across workers and returns records in the original order. */
  async extract(names: string[], bytes: Uint8Array, offsets: Uint32Array): Promise<string[]> {
    if (!this.workers.length) this.start();
    const n = names.length;
    const target = bytes.length / this.workers.length;
    const chunks: [number, number][] = [];
    let from = 0;
    for (let i = 1; i <= n; i++) {
      if (i === n || offsets[i] - offsets[from] >= target) {
        chunks.push([from, i]);
        from = i;
      }
    }
    const parts = await Promise.all(
      chunks.map(([a, b], i) => {
        const base = offsets[a];
        const request: ParseRequest = {
          id: this.nextId++,
          names: names.slice(a, b),
          bytes: bytes.slice(base, offsets[b]),
          offsets: offsets.slice(a, b + 1).map((o) => o - base),
        };
        return new Promise<string[]>((resolve) => {
          this.pending.set(request.id, resolve);
          this.workers[i % this.workers.length].postMessage(request, [request.bytes.buffer, request.offsets.buffer]);
        });
      }),
    );
    return parts.flat();
  }

  dispose() {
    for (const w of this.workers) w.terminate();
  }
}
