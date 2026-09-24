import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { test as base, type Page } from '@playwright/test';

export const REPO_DIR = fileURLToPath(new URL('../fixtures/synthetic/basic', import.meta.url));
export const REPO = 'example.test/synthetic/basic';

if (!existsSync(REPO_DIR)) execFileSync(fileURLToPath(new URL('../fixtures/make-repos.sh', import.meta.url)));

export const git = (...args: string[]) => execFileSync('git', ['-C', REPO_DIR, ...args], { encoding: 'utf8' }).trim();
export const firstParentShas = () => git('rev-list', '--first-parent', '--reverse', 'HEAD').split('\n');

const pkt = (s: string) => (s.length + 4).toString(16).padStart(4, '0') + s;

/** Serves the fixture repository over git smart HTTP (protocol v2) in place of the Cloudflare proxy. */
export async function serveGit(page: Page, status = 200) {
  await page.route('**/git/**', async (route) => {
    if (status !== 200) return route.fulfill({ status, body: 'Repository is private or does not exist' });
    const env = { ...process.env, GIT_PROTOCOL: 'version=2' };
    const advertise = route.request().method() === 'GET';
    const out = spawnSync('git', ['upload-pack', '--stateless-rpc', ...(advertise ? ['--advertise-refs'] : []), REPO_DIR], {
      env,
      input: advertise ? undefined : (route.request().postDataBuffer() ?? undefined),
      maxBuffer: 1 << 28,
    });
    const body = advertise ? Buffer.concat([Buffer.from(`${pkt('# service=git-upload-pack\n')}0000`), out.stdout]) : out.stdout;
    await route.fulfill({ body, contentType: `application/x-git-upload-pack-${advertise ? 'advertisement' : 'result'}` });
  });
}

export const test = base.extend<{ gitServer: undefined }>({
  gitServer: [
    async ({ page }, use) => {
      await serveGit(page);
      await use();
    },
    { auto: true },
  ],
});

export { expect } from '@playwright/test';
