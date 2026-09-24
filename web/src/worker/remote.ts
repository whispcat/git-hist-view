import { Engine } from '../wasm/core/index.js';
import { AppError, type FromWorker } from './protocol';
import { parseRepoUrl } from './repoUrl';

const BLOB_LIMIT = 1 << 20;
const MAX_PACK_BYTES = 512 << 20;

const STATUS_ERRORS: Record<number, [AppError['code'], string]> = {
  400: ['input', 'That doesn’t look like a git repository URL'],
  401: ['auth', 'Repository is private or does not exist'],
  404: ['not_found', 'Repository not found'],
  429: ['rate', 'Too many requests, try again in a minute'],
};

async function request(url: string, body?: Uint8Array): Promise<Response> {
  let res: Response;
  try {
    res = await fetch(
      url,
      body && { method: 'POST', body: body as Uint8Array<ArrayBuffer>, headers: { 'content-type': 'application/x-git-upload-pack-request' } },
    );
  } catch {
    throw new AppError('network', 'Network error, check your connection');
  }
  if (res.ok) return res;
  const [code, fallback] = STATUS_ERRORS[res.status] ?? ['network', `Git server error (${res.status})`];
  throw new AppError(code, (await res.text()) || fallback);
}

/** GitHub reports repository size up front, so hopeless downloads fail fast (no proxy needed: the API allows CORS). */
async function preflightGitHub(path: string) {
  const res = await fetch(`https://api.github.com/repos/${path}`).catch(() => null);
  if (!res?.ok) return;
  const { size } = (await res.json()) as { size?: number };
  if (size && size * 1024 > MAX_PACK_BYTES * 2) {
    throw new AppError('too_big', `This repository is ${Math.round(size / 1024)} MB, too large to analyze in the browser`);
  }
}

/** Fetches HEAD's history into the engine; returns the default branch name. */
export async function fetchRemote(engine: Engine, url: string, depth: number, post: (m: FromWorker) => void): Promise<string | undefined> {
  const repo = parseRepoUrl(url);
  if (!repo) throw new AppError('input', 'Enter a repository like owner/repo or https://host/owner/repo');
  const base = `/git/${repo.host}/${repo.path}.git`;

  post({ type: 'progress', phase: 'refs', done: 0 });
  if (repo.host === 'github.com') await preflightGitHub(repo.path);
  const advert = await request(`${base}/info/refs?service=git-upload-pack`);
  engine.set_capabilities(new Uint8Array(await advert.arrayBuffer()));
  const lsRefs = await request(`${base}/git-upload-pack`, Engine.ls_refs_request());
  const branch = engine.set_remote_head(new Uint8Array(await lsRefs.arrayBuffer()));

  const res = await request(`${base}/git-upload-pack`, engine.fetch_request(depth, BLOB_LIMIT, 0));
  const reader = res.body!.getReader();
  let received = 0;
  for (let r = await reader.read(); !r.done; r = await reader.read()) {
    received += r.value.byteLength;
    if (received > MAX_PACK_BYTES) {
      await reader.cancel();
      throw new AppError('too_big', `Download exceeded ${MAX_PACK_BYTES >> 20} MB, too large to analyze in the browser`);
    }
    const messages = engine.fetch_push(r.value);
    post({ type: 'progress', phase: 'download', done: received, message: messages.at(-1) });
  }
  engine.fetch_finish((done: number, total: number) => post({ type: 'progress', phase: 'index', done, total }));
  return branch;
}
