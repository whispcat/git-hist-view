import init, { Engine } from '../wasm/core/index.js';
import wasmUrl from '../wasm/core/index_bg.wasm?url';
import { loadLocal } from './local';
import { ParsePool } from './parsePool';
import { AppError, type CouplingQuery, type DepsQuery, type FromWorker, type Meta, type PathsDelta, type ToWorker, type TreemapQuery } from './protocol';
import { fetchRemote } from './remote';

const SLICE_MS = 30;
const STEP_BUDGET = 20_000;

const post = (m: FromWorker, transfer: Transferable[] = []) => postMessage(m, { transfer });
const ready = init({ module_or_path: wasmUrl });

// MessageChannel yields to the event loop without setTimeout's 4ms clamp.
const channel = new MessageChannel();
const yieldTask = () =>
  new Promise<void>((resolve) => {
    channel.port1.onmessage = () => resolve();
    channel.port2.postMessage(null);
  });

function people(engine: Engine, from: number) {
  const names = engine.author_names(from);
  const emails = engine.author_emails(from);
  const commits = engine.author_commits(from);
  return names.map((name, i) => ({ name, email: emails[i], commits: commits[i] }));
}

function meta(engine: Engine, branch: string | null): Meta {
  return {
    branch,
    oids: engine.commit_oids(),
    times: engine.commit_times(),
    authors: engine.commit_authors(),
    subjects: engine.commit_subjects(),
    people: people(engine, 0),
    keyframes: engine.keyframes(),
    truncated: engine.truncated(),
    languages: Engine.languages(),
    languageRank: engine.language_rank(),
    groupRank: engine.group_rank(),
  };
}

let engine: Engine | null = null;
let sentPaths = 0;

function pathsDelta(e: Engine): PathsDelta | null {
  const count = e.path_count();
  if (count === sentPaths) return null;
  const delta = { start: sentPaths, names: e.path_names(sentPaths), parents: e.path_parents(sentPaths) };
  sentPaths = count;
  return delta;
}

/** Answers a view request; data is null when the keyframe isn't analyzed yet (the view asks again later). */
function reply<T extends { held: boolean }>(type: 'treemap' | 'coupling' | 'deps', id: number, build: (e: Engine) => T) {
  let data: T | null = null;
  try {
    data = engine && build(engine);
  } catch {
    data = null;
  }
  const paths = engine && pathsDelta(engine);
  const transfer: Transferable[] = data
    ? (Object.values(data) as unknown[]).filter((v): v is ArrayBufferView => ArrayBuffer.isView(v)).map((v) => v.buffer as ArrayBuffer)
    : [];
  if (paths) transfer.push(paths.parents.buffer);
  post({ type, id, data, paths } as FromWorker, transfer);
}

function treemap(id: number, q: TreemapQuery) {
  reply('treemap', id, (e) => {
    const s = e.treemap(q.k, q.root, q.width, q.height, q.windowDays);
    const data = { ...q, ids: s.ids, geom: s.geom, loc: s.loc, churn: s.churn, owner: s.owner, lang: s.lang, flags: s.flags, held: s.held };
    s.free();
    return data;
  });
}

type Graph = ReturnType<Engine['coupling']>;

const graphData = (k: number, s: Graph) => {
  const data = {
    k,
    ids: s.ids,
    path: s.path,
    pos: s.pos,
    size: s.size,
    group: s.group,
    dir: s.dir,
    ends: s.ends,
    count: s.count,
    weight: s.weight,
    held: s.held,
    expanded: s.expanded,
  };
  s.free();
  return data;
};

function coupling(id: number, q: CouplingQuery) {
  reply('coupling', id, (e) => ({ ...graphData(q.k, e.coupling(q.k, q.windowDays)), windowDays: q.windowDays }));
}

const PARSE_BATCH_BYTES = 4 << 20;
let pool: ParsePool | null = null;
// Dependency requests run one at a time so concurrent prefetches never parse the same files twice.
let depsQueue = Promise.resolve();

function deps(id: number, q: DepsQuery) {
  depsQueue = depsQueue.then(async () => {
    const e = engine;
    try {
      let total = 0;
      for (;;) {
        const batch = e?.deps_batch(q.k, PARSE_BATCH_BYTES);
        if (!e || !batch?.names.length) break;
        total ||= batch.remaining;
        post({ type: 'parsing', done: total - batch.remaining, total });
        pool ??= new ParsePool();
        e.deps_insert(await pool.extract(batch.names, batch.bytes, batch.offsets));
        batch.free();
      }
      if (total) post({ type: 'parsing', done: total, total });
    } catch {
      // Not analyzed yet; the reply below is empty and the view asks again later.
    }
    reply('deps', id, (e) => graphData(q.k, e.deps(q.k, q.expanded ? Uint32Array.from(q.expanded) : e.deps_auto_expand(q.k))));
  });
}

async function analyze(engine: Engine, total: number, knownPeople: number) {
  let done = 0;
  while (done < total) {
    const start = performance.now();
    let next = done;
    while (next < total && performance.now() - start < SLICE_MS) next = engine.step(STEP_BUDGET);
    const churn = engine.churn_from(done);
    const fresh = people(engine, knownPeople);
    knownPeople += fresh.length;
    post({ type: 'frontier', done: next, churn, people: fresh }, [churn.buffer]);
    done = next;
    await yieldTask();
  }
}

addEventListener('message', async ({ data }: MessageEvent<ToWorker>) => {
  if (data.type === 'treemap') return treemap(data.id, data);
  if (data.type === 'coupling') return coupling(data.id, data);
  if (data.type === 'deps') return deps(data.id, data);
  try {
    await ready;
    const e = new Engine();
    const branch = data.source.kind === 'remote' ? await fetchRemote(e, data.source.url, data.maxCommits, post) : await loadLocal(e, data.source, post);
    post({ type: 'progress', phase: 'walk', done: 0 });
    const total = e.open_history(data.maxCommits);
    if (total === 0) throw new AppError('input', 'This repository has no commits');
    const m = meta(e, branch ?? null);
    post({ type: 'meta', meta: m }, [m.oids.buffer, m.times.buffer, m.authors.buffer, m.keyframes.buffer, m.languageRank.buffer, m.groupRank.buffer]);
    engine = e;
    await analyze(e, total, m.people.length);
  } catch (e) {
    const code = e instanceof AppError ? e.code : 'internal';
    post({ type: 'error', code, message: e instanceof Error ? e.message : String(e) });
  }
});
