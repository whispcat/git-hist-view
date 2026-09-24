import type { Engine } from '../wasm/core/index.js';
import { AppError, type FromWorker, type Source } from './protocol';

interface RepoFs {
  read(path: string): Promise<Uint8Array | null>;
  list(dir: string): Promise<string[]>;
}

function handleFs(root: FileSystemDirectoryHandle): RepoFs {
  const dir = async (path: string) => {
    let d = root;
    for (const part of path.split('/').filter(Boolean)) d = await d.getDirectoryHandle(part);
    return d;
  };
  return {
    async read(path) {
      const parts = path.split('/');
      try {
        const file = await (await dir(parts.slice(0, -1).join('/'))).getFileHandle(parts.at(-1)!);
        return new Uint8Array(await (await file.getFile()).arrayBuffer());
      } catch {
        return null;
      }
    },
    async list(path) {
      try {
        const names: string[] = [];
        for await (const name of (await dir(path)).keys()) names.push(name);
        return names;
      } catch {
        return [];
      }
    },
  };
}

function filesFs(files: { path: string; file: File }[]): RepoFs {
  const byPath = new Map(files.map((f) => [f.path, f.file]));
  return {
    async read(path) {
      const f = byPath.get(path);
      return f ? new Uint8Array(await f.arrayBuffer()) : null;
    },
    async list(dir) {
      const prefix = dir ? `${dir}/` : '';
      const names = new Set<string>();
      for (const p of byPath.keys()) if (p.startsWith(prefix)) names.add(p.slice(prefix.length).split('/')[0]);
      return [...names];
    },
  };
}

/** Narrows the picked folder to its git dir: a worktree root (`.git/`), a `.git` folder, or a bare repo. */
function scoped(fs: RepoFs, prefix: string): RepoFs {
  const join = (p: string) => (prefix ? `${prefix}/${p}` : p);
  return { read: (p) => fs.read(join(p)), list: (d) => fs.list(join(d)) };
}

async function gitDir(fs: RepoFs): Promise<RepoFs> {
  if ((await fs.list('.git')).includes('HEAD')) return scoped(fs, '.git');
  if ((await fs.read('HEAD')) && (await fs.list('objects')).length) return fs;
  if (await fs.read('.git')) throw new AppError('unsupported', 'Linked worktrees are not supported; pick the main repository');
  throw new AppError('input', 'No git repository found in that folder');
}

/** Loads a dropped or picked repository into the engine; returns the checked-out branch name. */
export async function loadLocal(engine: Engine, source: Exclude<Source, { kind: 'remote' }>, post: (m: FromWorker) => void): Promise<string | undefined> {
  const fs = await gitDir(source.kind === 'handle' ? handleFs(source.handle) : filesFs(source.files));
  if ((await fs.list('reftable')).length) throw new AppError('unsupported', 'Repositories using reftable are not supported yet');
  if (await fs.read('objects/info/alternates')) throw new AppError('unsupported', 'Repositories with alternates are not supported');

  const symbolic = engine.set_local_head((await fs.read('HEAD'))!);
  if (symbolic) engine.resolve_ref(symbolic, (await fs.read(symbolic)) ?? undefined, (await fs.read('packed-refs')) ?? undefined);

  const packs = (await fs.list('objects/pack')).filter((n) => n.endsWith('.pack'));
  for (const [i, name] of packs.entries()) {
    post({ type: 'progress', phase: 'read', done: i, total: packs.length });
    const [pack, idx] = await Promise.all([fs.read(`objects/pack/${name}`), fs.read(`objects/pack/${name.replace(/\.pack$/, '.idx')}`)]);
    if (pack && idx) engine.add_pack(pack, idx);
  }
  const fanout = (await fs.list('objects')).filter((d) => /^[0-9a-f]{2}$/.test(d));
  for (const [i, dir] of fanout.entries()) {
    post({ type: 'progress', phase: 'read', done: i, total: fanout.length, message: 'loose objects' });
    for (const name of await fs.list(`objects/${dir}`)) {
      const data = await fs.read(`objects/${dir}/${name}`);
      if (data) engine.add_loose(dir + name, data);
    }
  }
  return symbolic?.replace(/^refs\/heads\//, '');
}
