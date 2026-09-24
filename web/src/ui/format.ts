const dateFormat = new Intl.DateTimeFormat(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
const numberFormat = new Intl.NumberFormat();

export const formatDate = (seconds: number) => dateFormat.format(seconds * 1000);
export const formatNumber = (n: number) => numberFormat.format(n);

export function formatBytes(n: number) {
  if (n < 1 << 20) return `${Math.max(1, Math.round(n / 1024))} KB`;
  return `${(n / (1 << 20)).toFixed(1)} MB`;
}

export function shortSha(oids: Uint8Array, commit: number, length = 7) {
  let hex = '';
  for (let i = commit * 20; hex.length < length; i++) hex += oids[i].toString(16).padStart(2, '0');
  return hex.slice(0, length);
}

export function findCommit(oids: Uint8Array, prefix: string) {
  const n = oids.length / 20;
  for (let c = 0; c < n; c++) if (shortSha(oids, c, prefix.length) === prefix) return c;
  return -1;
}
