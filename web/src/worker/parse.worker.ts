import init, { Parser } from '../wasm/parse/index.js';
import wasmUrl from '../wasm/parse/index_bg.wasm?url';

export interface ParseRequest {
  id: number;
  names: string[];
  bytes: Uint8Array;
  offsets: Uint32Array;
}

const parser = init({ module_or_path: wasmUrl }).then(() => new Parser());

addEventListener('message', async ({ data }: MessageEvent<ParseRequest>) => {
  const records = (await parser).extract(data.names, data.bytes, data.offsets);
  postMessage({ id: data.id, records });
});
