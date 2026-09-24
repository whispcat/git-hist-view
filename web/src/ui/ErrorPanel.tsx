import type { ErrorCode } from '../worker/protocol';

const TITLES: Record<ErrorCode, string> = {
  input: 'Can’t open that',
  not_found: 'Repository not found',
  auth: 'Can’t access this repository',
  rate: 'Too many requests',
  too_big: 'Too large for the browser',
  unsupported: 'Unsupported repository',
  network: 'Network problem',
  internal: 'Something went wrong',
};

const HINTS: Partial<Record<ErrorCode, string>> = {
  auth: 'Only public repositories can be fetched. For a private one, open a local clone instead.',
  too_big: 'Try a smaller repository, or open a local clone.',
  rate: 'Wait a minute, then try again.',
};

export function ErrorPanel(props: { code: ErrorCode; message: string; onRetry: () => void; onBack: () => void }) {
  return (
    <div class="center-panel">
      <div class="panel" role="alert">
        <p class="error-title">{TITLES[props.code]}</p>
        <p class="error-message">{props.message}</p>
        {HINTS[props.code] && <p class="error-message">{HINTS[props.code]}</p>}
        <div class="actions">
          {props.code !== 'input' && props.code !== 'not_found' && (
            <button type="button" class="btn primary" onClick={props.onRetry}>
              Try again
            </button>
          )}
          <button type="button" class="btn" onClick={props.onBack}>
            Choose another
          </button>
        </div>
      </div>
    </div>
  );
}
