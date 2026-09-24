import { createEffect, createMemo, createSignal, Match, on, onCleanup, onMount, Show, Switch } from 'solid-js';
import { installKeys } from '../state/keys';
import * as session from '../state/session';
import { theme, toggleTheme } from '../state/theme';
import { commitAt, playing, pos, posOfCommit, readyFrames, seek, stepBy, toEnd, togglePlay, toStart } from '../state/timeline';
import { COLOR_MODES, type ColorMode, type CouplingWindow, readHash, VIEWS, type View, WINDOWS, writeHash } from '../state/url';
import { canonicalRepo, parseRepoUrl } from '../worker/repoUrl';
import { Announcer, announce } from './Announcer';
import { CouplingView, WINDOW_TEXT } from './CouplingView';
import { DepsView } from './DepsView';
import { ErrorPanel } from './ErrorPanel';
import { findCommit, formatNumber, shortSha } from './format';
import { Header, VIEW_LABELS } from './Header';
import { Help } from './Help';
import { EXAMPLES, Landing } from './Landing';
import { Loading } from './Loading';
import type { LocalRepo } from './localRepo';
import { type Command, Palette } from './Palette';
import { Scrubber } from './Scrubber';
import { MODE_LABELS, TreemapView } from './TreemapView';

const HASH_DEBOUNCE_MS = 400;
/** "Load all" still stops somewhere so a pathological history can't exhaust memory. */
const FULL_HISTORY_CAP = 200_000;

export function App() {
  const initial = readHash();
  const [view, setView] = createSignal<View>(initial.view ?? 'treemap');
  const [color, setColor] = createSignal<ColorMode>(initial.color ?? 'churn');
  const [couplingWindow, setCouplingWindow] = createSignal<CouplingWindow>(initial.window ?? '6m');
  const [treePath, setTreePath] = createSignal(initial.path ?? '');
  const [cap, setCap] = createSignal(initial.cap ?? session.defaultCap());
  const [table, setTable] = createSignal(false);
  const [help, setHelp] = createSignal(false);
  const [palette, setPalette] = createSignal(false);
  let pendingAt = initial.at;

  const openRemote = (input: string, push = true) => {
    const repo = parseRepoUrl(input);
    if (!repo) return announce('Enter a repository like owner/repo or https://host/owner/repo');
    const name = canonicalRepo(repo);
    if (push) setTreePath('');
    session.open({ kind: 'remote', url: name }, name, name, cap());
    if (push) writeHash({ repo: name, view: view() }, true);
  };

  const openLocal = (repo: LocalRepo) => {
    pendingAt = undefined;
    setTreePath('');
    session.open(repo.source, repo.name, undefined, cap());
    writeHash({ view: view() }, true);
  };

  const goHome = () => {
    session.close();
    writeHash({}, true);
  };

  const cycleColor = () => {
    const next = COLOR_MODES[(COLOR_MODES.indexOf(color()) + 1) % COLOR_MODES.length];
    setColor(next);
    announce(`Colored by ${MODE_LABELS[next].toLowerCase()}`);
  };

  const selectView = (v: View) => {
    setView(v);
    announce(`${VIEW_LABELS[v]} view`);
  };

  const toggleTable = () => {
    setTable(!table());
    announce(table() ? 'Showing the data as a table' : 'Showing the chart');
  };

  const loadAll = () => {
    setCap(FULL_HISTORY_CAP);
    session.reopen(FULL_HISTORY_CAP);
  };

  const focusRepoInput = () => (document.getElementById('repo-input') ?? document.getElementById('landing-input'))?.focus();

  const copyLink = () =>
    navigator.clipboard.writeText(location.href).then(
      () => announce('Link copied'),
      () => announce('Could not copy the link'),
    );

  const truncated = () => session.isAnalyzed() && !!session.meta()?.truncated;

  const notice = () => (
    <span class="view-notice">
      {!session.isAnalyzed()
        ? `Analyzing ${formatNumber(session.frontier())} / ${formatNumber(session.commitCount())} commits`
        : truncated()
          ? `Latest ${formatNumber(session.commitCount())} commits`
          : `${formatNumber(session.commitCount())} commits`}
      <Show when={truncated()}>
        {' · '}
        <button type="button" class="link" onClick={loadAll}>
          Load all
        </button>
      </Show>
    </span>
  );

  const controls = () => (
    <>
      {notice()}
      <button type="button" class="mode table-toggle" aria-pressed={table()} onClick={toggleTable} title="Show the data as a table">
        Table
      </button>
    </>
  );

  const showTreemap = (c: ColorMode) => {
    setView('treemap');
    setColor(c);
  };

  const showCoupling = (w: CouplingWindow) => {
    setView('coupling');
    setCouplingWindow(w);
  };

  const togglePalette = () => {
    setHelp(false);
    setPalette(!palette());
  };

  const commands = createMemo((): Command[] => {
    const list: Command[] = [];
    if (session.meta()) {
      for (const [i, v] of VIEWS.entries()) {
        list.push({ id: `view-${v}`, group: 'View', label: VIEW_LABELS[v], keys: String(i + 1), run: () => selectView(v) });
      }
      list.push({ id: 'table', group: 'View', label: table() ? 'Show the chart' : 'Show the data as a table', run: toggleTable });
      for (const c of COLOR_MODES) {
        list.push({ id: `color-${c}`, group: 'Treemap', label: `Color by ${MODE_LABELS[c].toLowerCase()}`, keys: 'C', run: () => showTreemap(c) });
      }
      if (treePath()) list.push({ id: 'tree-root', group: 'Treemap', label: 'Back to the repository root', run: () => setTreePath('') });
      for (const w of WINDOWS) {
        list.push({ id: `window-${w}`, group: 'Coupling', label: `Changes in the last ${WINDOW_TEXT[w]}`, run: () => showCoupling(w) });
      }
      list.push(
        { id: 'play', group: 'Timeline', label: playing() ? 'Pause' : 'Play history', keys: 'Space', run: togglePlay },
        { id: 'start', group: 'Timeline', label: 'Jump to the first commit', keys: 'Home', run: toStart },
        { id: 'end', group: 'Timeline', label: 'Jump to the latest commit', keys: 'End', run: toEnd },
        { id: 'copy', group: 'Share', label: 'Copy a link to this view', run: copyLink },
      );
      if (truncated()) list.push({ id: 'all', group: 'History', label: 'Load the full history', run: loadAll });
    }
    list.push({ id: 'repo', group: 'Repository', label: 'Open another repository', keys: '/', run: focusRepoInput });
    for (const r of EXAMPLES) list.push({ id: `example-${r}`, group: 'Repository', label: `Open ${r}`, run: () => openRemote(r) });
    list.push(
      { id: 'theme', group: 'Appearance', label: theme() === 'dark' ? 'Switch to light theme' : 'Switch to dark theme', keys: 'T', run: toggleTheme },
      { id: 'help', group: 'Help', label: 'Keyboard shortcuts', keys: '?', run: () => setHelp(true) },
    );
    return list;
  });

  onMount(() => {
    if (initial.repo) openRemote(initial.repo, false);
    const onHashChange = () => {
      const h = readHash();
      setView(h.view ?? 'treemap');
      setColor(h.color ?? 'churn');
      setCouplingWindow(h.window ?? '6m');
      setTreePath(h.path ?? '');
      if (h.cap) setCap(h.cap);
      if (h.repo && h.repo !== session.remote()) {
        pendingAt = h.at;
        openRemote(h.repo, false);
      } else if (!h.repo && session.remote()) session.close();
    };
    addEventListener('popstate', onHashChange);
    onCleanup(() => removeEventListener('popstate', onHashChange));
  });

  // A shared link names a commit; jump there once history has streamed past it.
  createEffect(() => {
    const m = session.meta();
    if (!m || !pendingAt) return;
    const c = findCommit(m.oids, pendingAt);
    if (c < 0) {
      pendingAt = undefined;
      return;
    }
    const target = Math.round(posOfCommit(c));
    if (readyFrames() > target) {
      pendingAt = undefined;
      seek(target);
    }
  });

  let hashTimer: ReturnType<typeof setTimeout> | undefined;
  createEffect(() => {
    const m = session.meta();
    const state = {
      repo: session.remote(),
      view: view(),
      color: color() === 'churn' ? undefined : color(),
      window: couplingWindow() === '6m' ? undefined : couplingWindow(),
      path: treePath() || undefined,
      at: m && !playing() ? shortSha(m.oids, commitAt(Math.round(pos()))) : undefined,
      cap: cap() === session.defaultCap() ? undefined : cap(),
    };
    if (session.status() === 'idle' || playing()) return;
    clearTimeout(hashTimer);
    hashTimer = setTimeout(() => writeHash(state), HASH_DEBOUNCE_MS);
  });

  createEffect(
    on(session.status, (s) => {
      if (s === 'error') announce(`Error: ${session.error()?.message}`);
      if (s === 'ready') announce(`Loaded ${formatNumber(session.commitCount())} commits from ${session.label()}`);
    }),
  );
  createEffect(on(playing, (p, prev) => prev !== undefined && announce(p ? 'Playing' : 'Paused')));

  onMount(() =>
    onCleanup(
      installKeys([
        ...VIEWS.map((v, i) => ({ key: String(i + 1), run: () => session.status() !== 'idle' && selectView(v) })),
        { key: ' ', run: () => session.meta() && togglePlay() },
        { key: 'ArrowLeft', run: (e) => stepBy(e.shiftKey ? -10 : -1) },
        { key: 'ArrowRight', run: (e) => stepBy(e.shiftKey ? 10 : 1) },
        { key: 'Home', run: toStart },
        { key: 'End', run: toEnd },
        { key: 't', run: toggleTheme },
        { key: 'c', run: () => session.meta() && view() === 'treemap' && cycleColor() },
        { key: '/', run: focusRepoInput },
        { key: '?', run: () => setHelp(true) },
        { key: 'k', mod: true, inInputs: true, inDialogs: true, run: togglePalette },
        { key: 'Escape', inInputs: true, run: (e) => (e.target as HTMLElement).blur?.() },
      ]),
    ),
  );

  return (
    <div class="app">
      <Header repo={session.remote() ?? ''} view={view()} onView={selectView} onRepo={(v) => openRemote(v)} onHome={goHome} onHelp={() => setHelp(true)} />
      <main class="stage">
        <Show when={session.status() !== 'idle'}>
          <h1 class="sr-only">{session.label()} history</h1>
        </Show>
        <Switch>
          <Match when={session.status() === 'idle'}>
            <Landing onRepo={(v) => openRemote(v)} onLocal={openLocal} />
          </Match>
          <Match when={session.status() === 'error' && session.error()}>
            {(err) => <ErrorPanel code={err().code} message={err().message} onRetry={session.retry} onBack={goHome} />}
          </Match>
          <Match when={session.status() === 'loading'}>
            <Loading />
          </Match>
          <Match when={session.meta()}>
            <div id="view" class="view" role="tabpanel" aria-labelledby={`tab-${view()}`}>
              <Switch>
                <Match when={view() === 'treemap'}>
                  <TreemapView color={color()} onColor={setColor} rootPath={treePath()} onRootPath={setTreePath} controls={controls()} table={table()} />
                </Match>
                <Match when={view() === 'coupling'}>
                  <CouplingView window={couplingWindow()} onWindow={setCouplingWindow} controls={controls()} table={table()} />
                </Match>
                <Match when={view() === 'deps'}>
                  <DepsView controls={controls()} table={table()} />
                </Match>
              </Switch>
            </div>
          </Match>
        </Switch>
      </main>
      <Show when={session.meta()}>
        <Scrubber />
      </Show>
      <Help open={help()} onClose={() => setHelp(false)} />
      <Palette open={palette()} onClose={() => setPalette(false)} commands={commands()} />
      <Announcer />
    </div>
  );
}
