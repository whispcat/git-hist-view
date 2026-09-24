import AxeBuilder from '@axe-core/playwright';
import { expect, firstParentShas, REPO, REPO_DIR, serveGit, test } from './fixtures';

const commits = firstParentShas();

// Reduced motion disables CSS transitions, so axe never samples colors mid-transition after a theme switch.
test.beforeEach(({ page }) => page.emulateMedia({ reducedMotion: 'reduce' }));

async function openRepo(page: import('@playwright/test').Page) {
  await page.goto('/');
  await page.getByRole('textbox', { name: 'Repository' }).fill(REPO);
  await page.getByRole('button', { name: 'Visualize repository' }).click();
  await analyzed(page);
}

// The commit count sits in the view bar, which phones hide, so check its text rather than visibility.
const analyzed = (page: import('@playwright/test').Page) => expect(page.locator('.view-notice, .notice').first()).toHaveText(`${commits.length} commits`);

const slider = (page: import('@playwright/test').Page) => page.getByRole('slider', { name: 'Position in history' });

test('landing page is accessible in both themes', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: /Replay any git repository/ })).toBeVisible();
  for (const theme of ['light', 'dark']) {
    await page.evaluate((t) => (document.documentElement.dataset.theme = t), theme);
    const { violations } = await new AxeBuilder({ page }).analyze();
    expect(violations, theme).toEqual([]);
  }
});

test('opens a repository and streams its history', async ({ page }) => {
  await openRepo(page);
  await expect(slider(page)).toHaveAttribute('aria-valuemax', String(commits.length - 1));
  await expect(slider(page)).toHaveAttribute('aria-valuenow', String(commits.length - 1));
  await expect(page.locator('.commit .subject')).toHaveText('loose object');
  await expect(page).toHaveURL(new RegExp(`#r=${REPO}&v=treemap&t=${commits.at(-1)!.slice(0, 7)}`));
  const { violations } = await new AxeBuilder({ page }).analyze();
  expect(violations).toEqual([]);
});

test('keyboard drives the timeline, views and theme', async ({ page, isMobile }) => {
  test.skip(isMobile, 'hardware keyboard');
  await openRepo(page);
  await page.keyboard.press('Home');
  await expect(slider(page)).toHaveAttribute('aria-valuenow', '0');
  await expect(page.locator('.commit .subject')).toHaveText('initial');
  await page.keyboard.press('ArrowRight');
  await page.keyboard.press('Shift+ArrowRight');
  await expect(slider(page)).toHaveAttribute('aria-valuenow', '11');
  await page.keyboard.press('End');
  await page.keyboard.press('ArrowLeft');
  await expect(slider(page)).toHaveAttribute('aria-valuenow', String(commits.length - 2));

  await page.keyboard.press('2');
  await expect(page.getByRole('tab', { name: /Coupling/ })).toHaveAttribute('aria-selected', 'true');
  await expect(page).toHaveURL(/v=coupling/);

  const before = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  await page.keyboard.press('t');
  await expect.poll(() => page.evaluate(() => getComputedStyle(document.body).backgroundColor)).not.toBe(before);

  await page.keyboard.press('/');
  await expect(page.getByRole('textbox', { name: 'Repository' })).toBeFocused();
  await page.keyboard.press('2');
  await expect(page.getByRole('tab', { name: /Coupling/ })).toHaveAttribute('aria-selected', 'true');

  await page.keyboard.press('Escape');
  await page.keyboard.press('Home');
  await page.keyboard.press('Space');
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await expect.poll(async () => Number(await slider(page).getAttribute('aria-valuenow'))).toBeGreaterThan(0);
});

test('a shared link restores repository, view and commit', async ({ page }) => {
  const target = commits[7].slice(0, 7);
  await page.goto(`/#r=${REPO}&v=deps&t=${target}`);
  await expect(slider(page)).toHaveAttribute('aria-valuenow', '7');
  await expect(page.getByRole('tab', { name: /Dependencies/ })).toHaveAttribute('aria-selected', 'true');
  await expect(page).toHaveURL(new RegExp(`t=${target}`));
});

test('scrubbing with the pointer seeks', async ({ page }) => {
  await openRepo(page);
  const box = (await page.locator('.track').boundingBox())!;
  await page.mouse.click(box.x + 2, box.y + box.height / 2);
  await expect(slider(page)).toHaveAttribute('aria-valuenow', '0');
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  const mid = Number(await slider(page).getAttribute('aria-valuenow'));
  expect(Math.abs(mid - (commits.length - 1) / 2)).toBeLessThanOrEqual(1);
});

test('private repositories show a helpful error', async ({ page }) => {
  await page.unrouteAll();
  await serveGit(page, 401);
  await page.goto(`/#r=${REPO}`);
  await expect(page.getByRole('alert')).toContainText('Can’t access this repository');
  await page.getByRole('button', { name: 'Choose another' }).click();
  await expect(page.getByRole('heading', { name: /Replay any git repository/ })).toBeVisible();
});

test('opens a local repository folder', async ({ page, browserName }) => {
  test.skip(browserName === 'webkit', 'Playwright WebKit cannot upload directories');
  await page.goto('/');
  await page.locator('input[type=file]').setInputFiles(REPO_DIR);
  await analyzed(page);
});

test.describe('treemap', () => {
  const map = (page: import('@playwright/test').Page) => page.getByRole('application', { name: /Files of/ });

  test('renders, recolors and stays accessible', async ({ page }) => {
    await openRepo(page);
    await expect(page.locator('.legend')).toContainText('Lines changed, last 90 days');
    await page.getByRole('button', { name: 'Author', exact: true }).click();
    await expect(page.locator('.legend')).toContainText('Owns most lines');
    if (!(await page.locator('.legend').evaluate((e) => (e as HTMLDetailsElement).open))) await page.locator('.legend summary').click();
    await expect(page.getByRole('button', { name: 'Ada Lovelace' })).toBeVisible();
    await expect(page).toHaveURL(/c=author/);
    const { violations } = await new AxeBuilder({ page }).analyze();
    expect(violations).toEqual([]);
  });

  test('keyboard moves between cells and drills into folders', async ({ page, isMobile }) => {
    test.skip(isMobile, 'hardware keyboard');
    await openRepo(page);
    await expect(map(page)).toHaveAttribute('aria-busy', 'false');
    await map(page).focus();
    await page.keyboard.press('ArrowRight');
    await expect(page.getByRole('status')).not.toBeEmpty();
    // Walk right until a folder is focused, then open it.
    for (let i = 0; i < 12 && !(await page.getByRole('status').textContent())?.includes('folder'); i++) await page.keyboard.press('ArrowRight');
    const folder = (await page.getByRole('status').textContent())!.split(',')[0];
    await page.keyboard.press('Enter');
    await expect(page.getByRole('navigation', { name: 'Folder' })).toContainText(folder);
    await page.keyboard.press('Escape');
    await expect(page.getByRole('navigation', { name: 'Folder' }).getByRole('button')).toHaveCount(1);
  });

  test('hover shows file details', async ({ page, isMobile }) => {
    test.skip(isMobile, 'no hover on touch');
    await openRepo(page);
    const box = (await page.locator('.map').boundingBox())!;
    await page.mouse.move(box.x + box.width * 0.4, box.y + box.height * 0.4);
    await expect(page.locator('.cell-tip')).toContainText('lines');
  });
});

test.describe('timeline motion', () => {
  /** Playhead position in keyframes (the fixture has fewer commits than keyframes, so each commit is one); CSS keeps ~6 digits. */
  const playhead = (page: import('@playwright/test').Page) =>
    page.locator('.playhead').evaluate((el, n) => (parseFloat((el as HTMLElement).style.left) / 100) * (n - 1), commits.length);

  test('a click on the track settles exactly on a keyframe', async ({ page }) => {
    await openRepo(page);
    const box = (await page.locator('.track').boundingBox())!;
    for (const f of [0.137, 0.52, 0.81]) {
      await page.mouse.click(box.x + box.width * f, box.y + box.height / 2);
      await expect
        .poll(async () => {
          const p = await playhead(page);
          return Math.abs(p - Math.round(p));
        })
        .toBeLessThan(1e-4);
    }
  });

  test('held arrow keys glide continuously', async ({ page, isMobile }) => {
    test.skip(isMobile, 'hardware keyboard');
    await openRepo(page);
    await page.keyboard.press('Home');
    await expect.poll(async () => Math.abs(await playhead(page))).toBeLessThan(1e-4);
    await page.evaluate((n) => {
      const samples: [number, number][] = [];
      (window as unknown as { samples: typeof samples }).samples = samples;
      const tick = (now: number) => {
        samples.push([now, (parseFloat(document.querySelector<HTMLElement>('.playhead')!.style.left) / 100) * (n - 1)]);
        if (samples.length < 120) requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
    }, commits.length);
    // OS key repeat fires roughly every 33ms while a key is held.
    for (let i = 0; i < 12; i++) {
      await page.keyboard.press('ArrowRight');
      await page.waitForTimeout(33);
    }
    await expect.poll(async () => Math.abs((await playhead(page)) - 12)).toBeLessThan(1e-4);
    const samples = await page.evaluate(() => (window as unknown as { samples: [number, number][] }).samples);
    // Speed over 50ms windows: loaded browsers can hand out frames with near-identical timestamps.
    const speeds = samples.flatMap(([t0, p0], i) => {
      const j = samples.findIndex(([t], k) => k > i && t - t0 >= 50);
      return j < 0 ? [] : [((samples[j][1] - p0) / (samples[j][0] - t0)) * 1000];
    });
    expect(samples.slice(1).every(([, p], i) => p >= samples[i][1] - 1e-3)).toBe(true);
    // Never faster than a few times the repeat rate: no cuts or bursts.
    expect(Math.max(...speeds)).toBeLessThan(90);
  });
});

test.describe('coupling graph', () => {
  const graph = (page: import('@playwright/test').Page) => page.getByRole('application', { name: /Files that changed together/ });

  test('lists co-changing files for the selected node and stays accessible', async ({ page, isMobile }) => {
    test.skip(isMobile, 'hardware keyboard');
    await openRepo(page);
    await page.keyboard.press('2');
    await expect(graph(page)).toHaveAttribute('aria-busy', 'false');
    await graph(page).focus();
    await page.keyboard.press('ArrowRight');
    await expect(page.getByRole('status')).toContainText(/(main\.rs|gen\.txt), \d+ commits, changes with 1 files/);
    await page.keyboard.press('Enter');
    const panel = page.getByRole('complementary', { name: 'Coupled files' });
    await expect(panel).toContainText(/30× · \d+%/);
    await expect(panel).toContainText(/gen\.txt|main\.rs/);
    const { violations } = await new AxeBuilder({ page }).analyze();
    expect(violations).toEqual([]);
    await page.keyboard.press('Escape');
    await expect(panel).toBeHidden();
  });

  test('window switch is shareable and early history explains an empty graph', async ({ page }) => {
    await openRepo(page);
    await page.getByRole('tab', { name: /Coupling/ }).click();
    await page.getByRole('button', { name: '1 yr', exact: true }).click();
    await expect(page).toHaveURL(/v=coupling&w=1y/);
    await page.getByRole('slider', { name: 'Position in history' }).focus();
    await page.keyboard.press('Home');
    await expect(page.getByText(/No files changed together at least 3 times/)).toBeVisible();
  });
});

test.describe('dependency graph', () => {
  const graph = (page: import('@playwright/test').Page) => page.getByRole('application', { name: /what they import/ });

  test('resolves imports and lists them for a selected file', async ({ page, isMobile }) => {
    test.skip(isMobile, 'hardware keyboard');
    await openRepo(page);
    await page.keyboard.press('3');
    await expect(graph(page)).toHaveAttribute('aria-busy', 'false');
    await graph(page).focus();
    const status = page.getByRole('status');
    for (let i = 0; i < 12 && !(await status.textContent())?.startsWith('app.ts'); i++) await page.keyboard.press('ArrowRight');
    for (let i = 0; i < 12 && !(await status.textContent())?.startsWith('app.ts'); i++) await page.keyboard.press('ArrowLeft');
    await expect(status).toContainText('app.ts, 2 lines, imports 1, imported by 0');
    await page.keyboard.press('Enter');
    const panel = page.getByRole('complementary', { name: 'Dependencies' });
    await expect(panel).toContainText('Imports');
    await expect(panel).toContainText('util.ts');
    const { violations } = await new AxeBuilder({ page }).analyze();
    expect(violations).toEqual([]);
  });
});

test.describe('polish', () => {
  test('help lists shortcuts and returns focus when closed', async ({ page, isMobile }) => {
    test.skip(isMobile, 'hardware keyboard');
    await openRepo(page);
    await page.keyboard.press('?');
    const help = page.getByRole('dialog', { name: 'Keyboard shortcuts' });
    await expect(help).toBeVisible();
    await expect(help).toContainText('Command palette');
    const { violations } = await new AxeBuilder({ page }).analyze();
    expect(violations).toEqual([]);
    await page.keyboard.press('Escape');
    await expect(help).toBeHidden();
    // Shortcuts are inert while a dialog is open, and work again once it closes.
    await page.keyboard.press('2');
    await expect(page.getByRole('tab', { name: /Coupling/ })).toHaveAttribute('aria-selected', 'true');
  });

  test('command palette filters and runs commands', async ({ page, isMobile }) => {
    test.skip(isMobile, 'hardware keyboard');
    await openRepo(page);
    await page.keyboard.press('ControlOrMeta+k');
    const palette = page.getByRole('dialog', { name: 'Command palette' });
    await expect(palette).toBeVisible();
    await page.keyboard.type('depend');
    await expect(page.getByRole('option').first()).toHaveText(/Dependencies/);
    await page.keyboard.press('Enter');
    await expect(palette).toBeHidden();
    await expect(page.getByRole('tab', { name: /Dependencies/ })).toHaveAttribute('aria-selected', 'true');
  });

  test('every view can be read as a table', async ({ page }) => {
    await openRepo(page);
    await page.getByRole('button', { name: 'Table' }).click();
    const table = page.getByRole('table');
    await expect(table).toContainText('Files in');
    await expect(table.getByRole('row').filter({ hasText: 'src/main.rs' })).toHaveCount(1);
    const { violations } = await new AxeBuilder({ page }).analyze();
    expect(violations).toEqual([]);
    await page.getByRole('tab', { name: /Coupling/ }).click();
    await expect(page.getByRole('table')).toContainText('Commits together');
    await page.getByRole('tab', { name: /Dependencies/ }).click();
    await expect(page.getByRole('table').getByRole('row').filter({ hasText: 'app.ts' })).toContainText('util.ts');
  });

  test('too-large GitHub repositories fail fast with a clear message', async ({ page }) => {
    await page.route('https://api.github.com/repos/**', (route) => route.fulfill({ json: { size: 5_000_000 } }));
    await page.goto('/#r=someone/enormous');
    await expect(page.getByRole('alert')).toContainText('Too large for the browser');
  });

  test('a truncated history offers to load the rest', async ({ page }) => {
    await page.goto(`/#r=${REPO}&n=10`);
    const notice = page.locator('.view-notice').first();
    await expect(notice).toContainText('Latest 10 commits');
    await notice.getByRole('button', { name: 'Load all' }).click();
    await analyzed(page);
  });

  test('a shared link reopens the treemap inside a folder', async ({ page }) => {
    await page.goto(`/#r=${REPO}&v=treemap&p=src`);
    await expect(page.getByRole('navigation', { name: 'Folder' }).getByRole('button', { name: 'src' })).toHaveAttribute('aria-current', 'page');
  });

  test('tapping a cell on a phone opens a closable bottom sheet', async ({ page, isMobile }) => {
    test.skip(!isMobile, 'touch layout');
    await openRepo(page);
    const map = page.locator('.map');
    await expect(map).toHaveAttribute('aria-busy', 'false');
    const box = (await map.boundingBox())!;
    await page.touchscreen.tap(box.x + box.width * 0.3, box.y + box.height * 0.3);
    const close = page.getByRole('button', { name: 'Close details' });
    await expect(close).toBeVisible();
    const sheet = (await page.locator('.cell-tip.pinned').boundingBox())!;
    expect(sheet.y + sheet.height).toBeGreaterThan(box.y + box.height - 24);
    await close.tap();
    await expect(close).toBeHidden();
  });
});
