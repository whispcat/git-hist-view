/** Accepts `owner/repo`, `host/owner/repo`, full URLs, `.git` suffixes and web UI paths like `/tree/main`. */
export function parseRepoUrl(input: string): { host: string; path: string } | null {
  const trimmed = input
    .trim()
    .replace(/^https?:\/\//, '')
    .replace(/^git@([^:]+):/, '$1/')
    .replace(/\/+$/, '');
  const parts = trimmed.split('/').filter(Boolean);
  if (parts.length === 2 && !parts[0].includes('.')) parts.unshift('github.com');
  if (parts.length < 3 || !parts[0].includes('.')) return null;
  const [host, ...rest] = parts;
  const cut = rest.findIndex((p) => ['-', 'tree', 'blob', 'commits', 'src'].includes(p));
  const path = (cut > 1 ? rest.slice(0, cut) : rest).join('/').replace(/\.git$/, '');
  return { host: host.toLowerCase(), path };
}

/** Canonical short form used in labels and share links: `owner/repo` on GitHub, `host/path` elsewhere. */
export const canonicalRepo = (r: { host: string; path: string }) => (r.host === 'github.com' ? r.path : `${r.host}/${r.path}`);
