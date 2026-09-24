interface Env {
  ASSETS: Fetcher;
  RL?: RateLimit;
}

const SERVICE = 'git-upload-pack';
const MAX_REDIRECTS = 3;
/** The client stops far earlier; this only bounds what a misbehaving upstream can make the Worker relay. */
const MAX_BYTES = 1 << 30;
const PRIVATE_HOST = /^(localhost|.*\.(local|internal|localhost)|\d+\.\d+\.\d+\.\d+|\[.*\])$/i;

const fail = (status: number, message: string) =>
  new Response(message, { status, headers: { 'content-type': 'text/plain', 'x-content-type-options': 'nosniff' } });

function capped(body: ReadableStream<Uint8Array>) {
  let seen = 0;
  return body.pipeThrough(
    new TransformStream<Uint8Array, Uint8Array>({
      transform(chunk, out) {
        seen += chunk.byteLength;
        if (seen > MAX_BYTES) out.error(new Error('response too large'));
        else out.enqueue(chunk);
      },
    }),
  );
}

/** Only the two smart-HTTP endpoints a clone needs are allowed, so this can't act as an open proxy. */
function upstreamUrl(url: URL): URL | null {
  const match = url.pathname.match(/^\/git\/([a-z0-9.-]+)\/(.+?)\/(info\/refs|git-upload-pack)$/i);
  if (!match || PRIVATE_HOST.test(match[1]) || !match[1].includes('.')) return null;
  if (match[3] === 'info/refs' && url.searchParams.get('service') !== SERVICE) return null;
  const target = new URL(`https://${match[1]}/${match[2]}/${match[3]}`);
  if (match[3] === 'info/refs') target.search = `?service=${SERVICE}`;
  return target;
}

async function proxy(req: Request, target: URL): Promise<Response> {
  const post = req.method === 'POST';
  const init: RequestInit = {
    method: req.method,
    redirect: 'manual',
    headers: {
      'user-agent': 'git/2.47-git-hist-view',
      'git-protocol': 'version=2',
      ...(post && { 'content-type': `application/x-${SERVICE}-request` }),
    },
    body: post ? await req.arrayBuffer() : undefined,
  };
  let res = await fetch(target, init);
  // GitLab and others redirect `/repo/...` to `/repo.git/...`; follow only same-host HTTPS redirects.
  for (let i = 0; i < MAX_REDIRECTS && res.status >= 300 && res.status < 400; i++) {
    const next = new URL(res.headers.get('location') ?? '', target);
    if (next.protocol !== 'https:' || next.host !== target.host) return fail(502, 'Refusing cross-host redirect');
    target = next;
    res = await fetch(target, init);
  }
  const type = res.headers.get('content-type') ?? '';
  if (res.status === 401 || res.status === 403) return fail(401, 'Repository is private or does not exist');
  if (res.status === 404) return fail(404, 'Repository not found');
  if (!res.ok || !type.startsWith(`application/x-${SERVICE}-`)) return fail(502, 'Not a git smart-HTTP endpoint');
  return new Response(res.body && capped(res.body), {
    headers: { 'content-type': type, 'cache-control': 'no-store', 'x-content-type-options': 'nosniff', 'x-upstream-url': target.origin + target.pathname },
  });
}

export default {
  async fetch(req, env): Promise<Response> {
    const url = new URL(req.url);
    if (!url.pathname.startsWith('/git/')) return env.ASSETS.fetch(req);
    const target = upstreamUrl(url);
    const allowed = target && (req.method === 'GET') === target.pathname.endsWith('/info/refs') && ['GET', 'POST'].includes(req.method);
    if (!target || !allowed) return fail(400, 'Unsupported git request');
    const ip = req.headers.get('cf-connecting-ip') ?? 'local';
    if (env.RL && !(await env.RL.limit({ key: ip })).success) return fail(429, 'Too many requests, try again in a minute');
    return proxy(req, target);
  },
} satisfies ExportedHandler<Env>;
