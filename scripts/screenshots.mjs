// Captures the README screenshots and the link preview image from the local vite clone (fixtures/repos/vite.git), without network access.
// Usage: bun run screenshots  (needs `bun run wasm` and a clone: git clone --bare https://github.com/vitejs/vite fixtures/repos/vite.git)
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { chromium, devices } from '@playwright/test';
import { createServer } from 'vite';

const root = fileURLToPath(new URL('..', import.meta.url));
const repo = `${root}fixtures/repos/vite.git`;
const docs = `${root}docs/screenshots`;
const desktop = { viewport: { width: 1440, height: 900 } };
const pkt = (s) => (s.length + 4).toString(16).padStart(4, '0') + s;

const server = await createServer({ root: `${root}web`, logLevel: 'error', server: { port: 5175, strictPort: true } });
await server.listen();
const browser = await chromium.launch();

async function page(options) {
  const context = await browser.newContext({ deviceScaleFactor: 2, ...options });
  const p = await context.newPage();
  await p.route('https://api.github.com/**', (r) => r.abort());
  await p.route('**/git/**', async (route) => {
    const advertise = route.request().method() === 'GET';
    const res = spawnSync('git', ['upload-pack', '--stateless-rpc', ...(advertise ? ['--advertise-refs'] : []), repo], {
      env: { ...process.env, GIT_PROTOCOL: 'version=2' },
      input: advertise ? undefined : route.request().postDataBuffer(),
      maxBuffer: 1 << 30,
    });
    const body = advertise ? Buffer.concat([Buffer.from(`${pkt('# service=git-upload-pack\n')}0000`), res.stdout]) : res.stdout;
    await route.fulfill({ body, contentType: `application/x-git-upload-pack-${advertise ? 'advertisement' : 'result'}` });
  });
  return p;
}

async function shot(path, hash, device = desktop) {
  const p = await page({ ...device, colorScheme: 'dark' });
  await p.addInitScript(() => localStorage.setItem('ghv:theme', 'dark'));
  await p.goto(`http://localhost:5175/${hash}`);
  await p.locator('.view-notice').first().filter({ hasText: 'commits', hasNotText: 'Analyzing' }).waitFor({ timeout: 120_000 });
  await p.locator('.map[aria-busy="false"]').waitFor({ timeout: 120_000 });
  await p.mouse.move(0, 0);
  await p.waitForTimeout(1200);
  await p.screenshot({ path, type: 'jpeg', quality: 86 });
  await p.context().close();
  console.log(`captured ${path.slice(root.length)}`);
}

await shot(`${docs}/treemap.jpg`, '#r=vitejs/vite&v=treemap');
await shot(`${docs}/coupling.jpg`, '#r=vitejs/vite&v=coupling');
await shot(`${docs}/dependencies.jpg`, '#r=vitejs/vite&v=deps');
await shot(`${docs}/mobile.jpg`, '#r=vitejs/vite&v=treemap&c=language', devices['iPhone 15']);
// Link previews are shown at 1200×630; a 1x capture keeps labels legible and the file small.
await shot(`${root}web/public/og.jpg`, '#r=vitejs/vite&v=treemap', { viewport: { width: 1200, height: 630 }, deviceScaleFactor: 1 });

await browser.close();
await server.close();
