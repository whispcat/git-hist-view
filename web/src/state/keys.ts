interface Binding {
  /** `KeyboardEvent.key`, matched case-insensitively for letters. */
  key: string;
  shift?: boolean;
  mod?: boolean;
  /** Fires even while typing in a text field. */
  inInputs?: boolean;
  /** Fires even while a modal dialog is open. */
  inDialogs?: boolean;
  run: (e: KeyboardEvent) => void;
}

const typing = (t: EventTarget | null) => t instanceof HTMLElement && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName));
const activates = (t: EventTarget | null) => t instanceof HTMLElement && /^(BUTTON|A|SUMMARY)$/.test(t.tagName);

/** One global dispatcher: shortcuts never fire while typing, and Space/Enter keep activating focused buttons. */
export function installKeys(bindings: Binding[]) {
  const onKey = (e: KeyboardEvent) => {
    if (e.defaultPrevented || e.altKey) return;
    const mod = e.metaKey || e.ctrlKey;
    if ((e.key === ' ' || e.key === 'Enter') && activates(e.target)) return;
    const b = bindings.find(
      (b) =>
        b.key.toLowerCase() === e.key.toLowerCase() &&
        !!b.mod === mod &&
        (b.shift === undefined || b.shift === e.shiftKey) &&
        (b.inInputs || !typing(e.target)),
    );
    if (!b || (document.querySelector('dialog[open]') && !b.inDialogs)) return;
    e.preventDefault();
    b.run(e);
  };
  addEventListener('keydown', onKey);
  return () => removeEventListener('keydown', onKey);
}
