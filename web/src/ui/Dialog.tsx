import { createEffect, type JSX } from 'solid-js';

/** A native modal dialog: focus moves in, Escape and backdrop clicks close it, focus returns on close. */
export function Dialog(props: { open: boolean; onClose: () => void; label: string; class?: string; onOpen?: () => void; children: JSX.Element }) {
  let ref!: HTMLDialogElement;
  createEffect(() => {
    if (props.open && !ref.open) {
      ref.showModal();
      props.onOpen?.();
    } else if (!props.open && ref.open) ref.close();
  });
  return (
    // biome-ignore lint/a11y/useKeyWithClickEvents: backdrop clicks close the dialog; the keyboard uses Escape
    <dialog ref={ref} class={`dialog ${props.class ?? ''}`} aria-label={props.label} onClose={props.onClose} onClick={(e) => e.target === ref && ref.close()}>
      {props.children}
    </dialog>
  );
}
