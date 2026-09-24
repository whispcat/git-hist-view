import { createSignal, For } from 'solid-js';
import { RepoForm } from './Header';
import { Folder } from './icons';
import { canPickFolder, fromDrop, fromFileList, type LocalRepo, pickFolder } from './localRepo';

export const EXAMPLES = ['sharkdp/hexyl', 'BurntSushi/ripgrep', 'pallets/flask', 'vitejs/vite'];

export function Landing(props: { onRepo: (value: string) => void; onLocal: (repo: LocalRepo) => void }) {
  const [dropping, setDropping] = createSignal(false);
  let fileInput!: HTMLInputElement;

  const openFolder = async () => {
    if (!canPickFolder()) return fileInput.click();
    const repo = await pickFolder();
    if (repo) props.onLocal(repo);
  };

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: dropping a folder is a pointer shortcut for the "choose a folder" button
    <section
      class="landing"
      classList={{ dropping: dropping() }}
      onDragOver={(e) => {
        e.preventDefault();
        setDropping(true);
      }}
      onDragLeave={(e) => e.currentTarget === e.target && setDropping(false)}
      onDrop={async (e) => {
        e.preventDefault();
        setDropping(false);
        const repo = e.dataTransfer && (await fromDrop(e.dataTransfer));
        if (repo) props.onLocal(repo);
      }}
    >
      <div class="landing-inner">
        <h1>
          Replay any git repository's <em>history</em>.
        </h1>
        <p class="lede">Treemaps, co-change coupling and module dependencies, animated commit by commit. Paste a link or open a local clone.</p>
        <RepoForm id="landing-input" value="" onSubmit={props.onRepo} large />
        <div class="alt-row">
          <span>Try</span>
          <For each={EXAMPLES}>
            {(repo) => (
              <button type="button" class="chip" onClick={() => props.onRepo(repo)}>
                {repo}
              </button>
            )}
          </For>
        </div>
        <div class="drop-hint">
          <Folder />
          <span>
            Drop a local repository here, or{' '}
            <button type="button" class="link" onClick={openFolder}>
              choose a folder
            </button>
          </span>
          <input
            ref={fileInput}
            type="file"
            webkitdirectory
            hidden
            onChange={(e) => {
              const repo = e.currentTarget.files && fromFileList(e.currentTarget.files);
              if (repo) props.onLocal(repo);
            }}
          />
        </div>
        <p class="privacy">Everything is analyzed in your browser. Remote repositories are fetched through a stateless proxy that only relays git traffic.</p>
      </div>
    </section>
  );
}
