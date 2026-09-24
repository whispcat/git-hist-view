import type { JSX } from 'solid-js';

const Svg = (props: { children: JSX.Element }) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {props.children}
  </svg>
);

export const Logo = () => (
  <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true">
    <path d="M6 5v14M6 12c0-4 4-5 12-5" fill="none" stroke="var(--ink-2)" stroke-width="1.75" stroke-linecap="round" />
    <circle cx="6" cy="5" r="2.25" fill="var(--ink-2)" />
    <circle cx="6" cy="19" r="2.25" fill="var(--ink-2)" />
    <circle cx="18" cy="7" r="3" fill="var(--accent)" />
  </svg>
);

export const Play = () => (
  <Svg>
    <path d="M8 5.5v13l10.5-6.5z" fill="currentColor" stroke="none" />
  </Svg>
);

export const Pause = () => (
  <Svg>
    <path d="M8.5 5.5v13M15.5 5.5v13" stroke-width="2.5" />
  </Svg>
);

export const Sun = () => (
  <Svg>
    <circle cx="12" cy="12" r="4" />
    <path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4" />
  </Svg>
);

export const Moon = () => (
  <Svg>
    <path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z" />
  </Svg>
);

export const Folder = () => (
  <Svg>
    <path d="M3.5 7.5a2 2 0 0 1 2-2h4l2 2h7a2 2 0 0 1 2 2v7.5a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z" />
  </Svg>
);

export const Arrow = () => (
  <Svg>
    <path d="M5 12h14M13 6l6 6-6 6" />
  </Svg>
);

export const Question = () => (
  <Svg>
    <circle cx="12" cy="12" r="9" />
    <path d="M9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.7.3-1 .8-1 1.5v.4M12 16.8v.2" />
  </Svg>
);

export const Close = () => (
  <Svg>
    <path d="M6 6l12 12M18 6L6 18" />
  </Svg>
);
