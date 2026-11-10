// Inline stroke icon set — 1.5px visual stroke, round caps, drawn by hand
// (no icon library). All paths live in a 24px viewBox; the .ico-* size
// classes in app.css thin the stroke so every render keeps 1.5px visually.

const svg = (body: string): string =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.25" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;

export const icons = {
  /** Sidebar · Devices */
  laptop: svg(
    `<rect x="4.5" y="4.5" width="15" height="10.5" rx="2"/>
     <path d="M2.5 19h19"/>`,
  ),
  /** Sidebar · Send */
  send: svg(
    `<circle cx="12" cy="12" r="9"/>
     <path d="M12 16V8"/>
     <path d="M8.6 11.4 12 8l3.4 3.4"/>`,
  ),
  /** Sidebar · Receive */
  receive: svg(
    `<circle cx="12" cy="12" r="9"/>
     <path d="M12 8v8"/>
     <path d="M8.6 12.6 12 16l3.4-3.4"/>`,
  ),
  /** Sidebar · Settings */
  gear: svg(
    `<circle cx="12" cy="12" r="3"/>
     <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/>`,
  ),
  /** Send · empty dropzone */
  upload: svg(
    `<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
     <path d="M17 8 12 3 7 8"/>
     <path d="M12 3v12"/>`,
  ),
  /** Receive · empty log */
  inbox: svg(
    `<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
     <path d="M7 10l5 5 5-5"/>
     <path d="M12 3v12"/>`,
  ),
  /** Devices · empty state */
  devicesSearch: svg(
    `<rect x="4.5" y="4.5" width="15" height="10.5" rx="2"/>
     <path d="M2.5 19h19"/>
     <circle cx="17.2" cy="17.2" r="3.6"/>
     <path d="m20 20 1.8 1.8"/>`,
  ),
  /** Folder (paths, targets) */
  folder: svg(
    `<path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>`,
  ),
  /** Copy */
  copy: svg(
    `<rect x="9" y="9" width="13" height="13" rx="2"/>
     <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>`,
  ),
  /** Success mark */
  check: svg(`<path d="M20 6 9 17l-5-5"/>`),
  /** Warning mark */
  alert: svg(
    `<path d="M12 8v5"/>
     <path d="M12 16.5v.01"/>`,
  ),
} as const;
