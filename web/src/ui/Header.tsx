import { createEffect, createSignal, For, Show } from 'solid-js';
import { status } from '../state/session';
import { theme, toggleTheme } from '../state/theme';
import { VIEWS, type View } from '../state/url';
import { Arrow, Logo, Moon, Question, Sun } from './icons';

export const VIEW_LABELS: Record<View, string> = { treemap: 'Treemap', coupling: 'Coupling', deps: 'Dependencies' };

export function RepoForm(props: { value: string; onSubmit: (value: string) => void; id?: string; large?: boolean }) {
  const [value, setValue] = createSignal(props.value);
  createEffect(() => setValue(props.value));
  return (
    // biome-ignore lint/a11y/useSemanticElements: <search> needs Safari 17; the role gives every browser the landmark
    <form
      class="repo-form"
      role="search"
      onSubmit={(e) => {
        e.preventDefault();
        if (value().trim()) props.onSubmit(value());
      }}
    >
      <label class="sr-only" for={props.id}>
        Repository
      </label>
      <input
        id={props.id}
        class="field"
        value={value()}
        onInput={(e) => setValue(e.currentTarget.value)}
        placeholder="owner/repo or https://host/owner/repo"
        autocomplete="off"
        autocapitalize="off"
        spellcheck={false}
        enterkeyhint="go"
      />
      <button class={props.large ? 'btn primary' : 'btn icon'} type="submit" aria-label="Visualize repository">
        <Show when={props.large} fallback={<Arrow />}>
          Visualize
        </Show>
      </button>
    </form>
  );
}

export function Header(props: { repo: string; view: View; onView: (v: View) => void; onRepo: (v: string) => void; onHome: () => void; onHelp: () => void }) {
  return (
    <header class="top">
      <button type="button" class="brand" onClick={props.onHome} aria-label="git-hist-view home">
        <Logo />
        <span>git-hist-view</span>
      </button>
      <Show when={status() !== 'idle'}>
        <RepoForm id="repo-input" value={props.repo} onSubmit={props.onRepo} />
        <div class="tabs" role="tablist" aria-label="Visualization">
          <For each={VIEWS}>
            {(v, i) => (
              <button
                type="button"
                class="tab"
                role="tab"
                id={`tab-${v}`}
                aria-selected={props.view === v}
                aria-controls="view"
                tabIndex={props.view === v ? 0 : -1}
                aria-keyshortcuts={String(i() + 1)}
                onClick={() => props.onView(v)}
                onKeyDown={(e) => {
                  const step = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0;
                  if (!step) return;
                  e.preventDefault();
                  const next = VIEWS[(i() + step + VIEWS.length) % VIEWS.length];
                  props.onView(next);
                  document.getElementById(`tab-${next}`)?.focus();
                }}
              >
                {VIEW_LABELS[v]}
                {/* biome-ignore lint/a11y/noAriaHiddenOnFocusable: the digit is announced via aria-keyshortcuts instead */}
                <kbd aria-hidden="true">{i() + 1}</kbd>
              </button>
            )}
          </For>
        </div>
      </Show>
      <button
        type="button"
        class="btn ghost icon help-toggle"
        style={status() === 'idle' ? { 'margin-left': 'auto' } : {}}
        onClick={props.onHelp}
        aria-label="Keyboard shortcuts"
        title="Keyboard shortcuts (?)"
      >
        <Question />
      </button>
      <button
        type="button"
        class="btn ghost icon theme-toggle"
        onClick={toggleTheme}
        aria-label={theme() === 'dark' ? 'Switch to light theme' : 'Switch to dark theme'}
        title="Toggle theme (T)"
      >
        <Show when={theme() === 'dark'} fallback={<Moon />}>
          <Sun />
        </Show>
      </button>
    </header>
  );
}
