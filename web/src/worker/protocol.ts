export type Source =
  | { kind: 'remote'; url: string }
  | { kind: 'handle'; handle: FileSystemDirectoryHandle }
  | { kind: 'files'; files: { path: string; file: File }[] };

export type Phase = 'refs' | 'download' | 'index' | 'read' | 'walk';

export type ErrorCode = 'input' | 'not_found' | 'auth' | 'rate' | 'too_big' | 'unsupported' | 'network' | 'internal';

export interface Person {
  name: string;
  email: string;
  commits: number;
}

export interface Meta {
  branch: string | null;
  /** 20 bytes per commit, oldest first. */
  oids: Uint8Array;
  /** Committer time in seconds, clamped to be non-decreasing. */
  times: Float64Array;
  authors: Uint32Array;
  subjects: string[];
  people: Person[];
  /** Commit indices of animation keyframes. */
  keyframes: Uint32Array;
  truncated: boolean;
  languages: string[];
  /** Language ids by file count at HEAD; fixes color slots for the whole session. */
  languageRank: Uint8Array;
  /** Top-level directories (0 = files at the root) by file count at HEAD. */
  groupRank: Uint32Array;
}

export interface CouplingQuery {
  k: number;
  windowDays: number;
}

export interface DepsQuery {
  k: number;
  /** Folders to show opened; null asks the engine to choose (the answer comes back in `expanded`). */
  expanded: number[] | null;
}

/** Node-link graph tweening from keyframe `k` to `k + 1`; per-node and per-edge values are `[A, B]` pairs. */
export interface GraphData {
  k: number;
  /** Stable node ids: file lineage for coupling, path ids for dependencies. */
  ids: Uint32Array;
  /** Path id to label each node with, at A and B (0 when absent). */
  path: Uint32Array;
  /** `[xA, yA, xB, yB]` in layout units. */
  pos: Float32Array;
  /** Radius driver: commits for coupling, lines of code for dependencies (0 when absent). */
  size: Uint32Array;
  group: Uint32Array;
  dir: Uint8Array;
  /** Endpoint node indices per edge; for dependencies, importer then imported. */
  ends: Uint32Array;
  count: Uint32Array;
  weight: Float32Array;
  held: boolean;
  expanded: Uint32Array;
}

export interface TreemapQuery {
  k: number;
  root: number;
  width: number;
  height: number;
  windowDays: number;
}

/** Cells tweening from keyframe `k` to `k + 1`: geometry is `[x, y, w, h]` at A then B; loc/churn/owner are `[A, B]` pairs. */
export interface TreemapData extends TreemapQuery {
  ids: Uint32Array;
  geom: Float32Array;
  loc: Uint32Array;
  churn: Uint32Array;
  owner: Uint32Array;
  lang: Uint8Array;
  /** bit 0: directory, bit 1: has a name header, bits 2+: depth below the root. */
  flags: Uint8Array;
  /** B repeats A because keyframe `k + 1` isn't analyzed yet; ask again later. */
  held: boolean;
}

/** Path table entries `[start, start + names.length)`; ids are dense and never change. */
export interface PathsDelta {
  start: number;
  names: string[];
  parents: Uint32Array;
}

export type ToWorker =
  | { type: 'open'; source: Source; maxCommits: number }
  | ({ type: 'treemap'; id: number } & TreemapQuery)
  | ({ type: 'coupling'; id: number } & CouplingQuery)
  | ({ type: 'deps'; id: number } & DepsQuery);

export type FromWorker =
  | { type: 'progress'; phase: Phase; done: number; total?: number; message?: string }
  | { type: 'meta'; meta: Meta }
  /** `churn` covers commits `[done - churn.length, done)`; `people` appends authors first seen on side branches. */
  | { type: 'frontier'; done: number; churn: Uint32Array; people: Person[] }
  | { type: 'treemap'; id: number; data: TreemapData | null; paths: PathsDelta | null }
  | { type: 'coupling' | 'deps'; id: number; data: GraphData | null; paths: PathsDelta | null }
  | { type: 'parsing'; done: number; total: number }
  | { type: 'error'; code: ErrorCode; message: string };

export class AppError extends Error {
  constructor(
    readonly code: ErrorCode,
    message: string,
  ) {
    super(message);
  }
}
