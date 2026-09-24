import { batch, createSignal } from 'solid-js';
import type { CouplingQuery, DepsQuery, ErrorCode, FromWorker, GraphData, Meta, Phase, Source, TreemapData, TreemapQuery } from '../worker/protocol';
import { appendPaths, resetPaths } from './paths';

type Status = 'idle' | 'loading' | 'ready' | 'error';

interface Progress {
  phase: Phase;
  done: number;
  total?: number;
  message?: string;
}

const [status, setStatus] = createSignal<Status>('idle');
const [label, setLabel] = createSignal('');
const [remote, setRemote] = createSignal<string | undefined>();
const [progress, setProgress] = createSignal<Progress | null>(null);
const [meta, setMeta] = createSignal<Meta | null>(null);
const [frontier, setFrontier] = createSignal(0);
const [error, setError] = createSignal<{ code: ErrorCode; message: string } | null>(null);
/** Import extraction progress for the dependency view. */
const [parsing, setParsing] = createSignal<{ done: number; total: number } | null>(null);

/** Per-commit churn, filled in place as history streams in; readers track `frontier`. */
let churn = new Uint32Array(0);
let worker: Worker | null = null;
let nextId = 0;
const pending = new Map<number, (data: unknown) => void>();
let lastSource: { source: Source; label: string; remote?: string; cap: number } | null = null;

export { error, frontier, label, meta, parsing, progress, remote, status };
export const commitChurn = () => churn;
export const commitCount = () => meta()?.times.length ?? 0;
export const isAnalyzed = () => status() === 'ready' && frontier() === commitCount();

export const defaultCap = () => (matchMedia('(pointer: coarse)').matches ? 2000 : 10000);

function onMessage(m: FromWorker) {
  switch (m.type) {
    case 'progress':
      setProgress(m);
      break;
    case 'meta':
      churn = new Uint32Array(m.meta.times.length);
      batch(() => {
        setMeta(m.meta);
        setStatus('ready');
      });
      break;
    case 'frontier':
      churn.set(m.churn, m.done - m.churn.length);
      meta()?.people.push(...m.people);
      setFrontier(m.done);
      break;
    case 'parsing':
      setParsing(m.done < m.total ? { done: m.done, total: m.total } : null);
      break;
    case 'treemap':
    case 'coupling':
    case 'deps':
      if (m.paths) appendPaths(m.paths);
      pending.get(m.id)?.(m.data);
      pending.delete(m.id);
      break;
    case 'error':
      stopWorker();
      batch(() => {
        setError({ code: m.code, message: m.message });
        setStatus('error');
      });
  }
}

function request<T>(type: 'treemap' | 'coupling' | 'deps', query: object): Promise<T | null> {
  if (!worker) return Promise.resolve(null);
  const id = nextId++;
  worker.postMessage({ type, id, ...query });
  return new Promise((resolve) => pending.set(id, resolve as (data: unknown) => void));
}

/** View segments resolve to null if that keyframe isn't analyzed yet. */
export const requestTreemap = (q: TreemapQuery) => request<TreemapData>('treemap', q);
export const requestCoupling = (q: CouplingQuery) => request<GraphData>('coupling', q);
export const requestDeps = (q: DepsQuery) => request<GraphData>('deps', q);

function stopWorker() {
  worker?.terminate();
  worker = null;
  for (const resolve of pending.values()) resolve(null);
  pending.clear();
}

export function open(source: Source, name: string, remoteUrl: string | undefined, cap: number) {
  stopWorker();
  resetPaths();
  lastSource = { source, label: name, remote: remoteUrl, cap };
  batch(() => {
    setLabel(name);
    setRemote(remoteUrl);
    setMeta(null);
    setFrontier(0);
    setProgress(null);
    setError(null);
    setParsing(null);
    setStatus('loading');
  });
  worker = new Worker(new URL('../worker/core.worker.ts', import.meta.url), { type: 'module' });
  worker.onmessage = ({ data }: MessageEvent<FromWorker>) => onMessage(data);
  worker.postMessage({ type: 'open', source, maxCommits: cap });
}

/** Reopens the current repository with a different commit cap. */
export function reopen(cap: number) {
  if (lastSource) open(lastSource.source, lastSource.label, lastSource.remote, cap);
}

export function retry() {
  if (lastSource) open(lastSource.source, lastSource.label, lastSource.remote, lastSource.cap);
}

export function close() {
  stopWorker();
  batch(() => {
    setStatus('idle');
    setMeta(null);
    setRemote(undefined);
    setLabel('');
  });
}
