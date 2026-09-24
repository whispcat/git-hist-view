import { createSignal } from 'solid-js';

const [message, setMessage] = createSignal('');

/** Politely announces a status change to screen readers. */
export function announce(text: string) {
  setMessage('');
  queueMicrotask(() => setMessage(text));
}

export const Announcer = () => (
  <div class="sr-only" role="status" aria-live="polite">
    {message()}
  </div>
);
