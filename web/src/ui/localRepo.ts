import type { Source } from '../worker/protocol';

type Entry = { path: string; file: File };

export interface LocalRepo {
  source: Source;
  name: string;
}

/** A working tree can be huge; when it contains `.git`, only that folder is needed. */
function onlyGitDir(entries: Entry[]): Entry[] {
  const git = entries.filter((e) => e.path.startsWith('.git/'));
  return git.length ? git : entries;
}

export async function pickFolder(): Promise<LocalRepo | null> {
  try {
    const handle = await showDirectoryPicker({ mode: 'read' });
    return { source: { kind: 'handle', handle }, name: handle.name };
  } catch {
    return null;
  }
}

export const canPickFolder = () => 'showDirectoryPicker' in window;

export function fromFileList(list: FileList): LocalRepo | null {
  const files = [...list];
  if (!files.length) return null;
  const name = files[0].webkitRelativePath.split('/')[0];
  const entries = files.map((file) => ({ path: file.webkitRelativePath.split('/').slice(1).join('/'), file }));
  return { source: { kind: 'files', files: onlyGitDir(entries) }, name };
}

async function readEntries(dir: FileSystemDirectoryEntry): Promise<FileSystemEntry[]> {
  const reader = dir.createReader();
  const all: FileSystemEntry[] = [];
  // readEntries returns results in batches until it yields an empty one.
  for (;;) {
    const batch = await new Promise<FileSystemEntry[]>((resolve, reject) => reader.readEntries(resolve, reject));
    if (!batch.length) return all;
    all.push(...batch);
  }
}

const fileOf = (e: FileSystemEntry) => new Promise<File>((resolve, reject) => (e as FileSystemFileEntry).file(resolve, reject));

async function collect(entries: FileSystemEntry[], prefix: string, out: Entry[]) {
  for (const e of entries) {
    const path = prefix + e.name;
    if (e.isDirectory) await collect(await readEntries(e as FileSystemDirectoryEntry), `${path}/`, out);
    else out.push({ path, file: await fileOf(e) });
  }
}

export async function fromDrop(dt: DataTransfer): Promise<LocalRepo | null> {
  const item = [...dt.items].find((i) => i.kind === 'file');
  if (!item) return null;
  const handle = await (item as DataTransferItem & { getAsFileSystemHandle?: () => Promise<FileSystemHandle | null> }).getAsFileSystemHandle?.();
  if (handle?.kind === 'directory') return { source: { kind: 'handle', handle: handle as FileSystemDirectoryHandle }, name: handle.name };
  const entry = item.webkitGetAsEntry();
  if (!entry?.isDirectory) return null;
  const children = await readEntries(entry as FileSystemDirectoryEntry);
  const git = children.find((c) => c.name === '.git' && c.isDirectory);
  const files: Entry[] = [];
  await collect(git ? [git] : children, '', files);
  return { source: { kind: 'files', files }, name: entry.name };
}
