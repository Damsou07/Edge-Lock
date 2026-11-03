import type { CSSProperties } from "react";

const paths = {
  cube: "m12 3 9 5v8l-9 5-9-5V8l9-5Zm0 0v9m9-4-9 4-9-4m9 4v9",
  grid: "M3 3h7v7H3zM14 3h7v7h-7zM3 14h7v7H3zM14 14h7v7h-7z",
  list: "M9 6h12M9 12h12M9 18h12M3 6h1M3 12h1M3 18h1",
  folder: "M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v10H3V7Z",
  plus: "M12 5v14M5 12h14",
  search: "m21 21-5-5M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0",
  import: "M12 3v12m-5-5 5 5 5-5M4 15v6h16v-6",
  export: "M12 15V3m-5 5 5-5 5 5M4 15v6h16v-6",
  settings:
    "M9.7 3h4.6l.5 2.2 1.7 1 2.2-.7 2.3 4-1.7 1.5v2l1.7 1.5-2.3 4-2.2-.7-1.7 1-.5 2.2H9.7l-.5-2.2-1.7-1-2.2.7-2.3-4 1.7-1.5v-2L3 9.5l2.3-4 2.2.7 1.7-1L9.7 3Zm5.3 9a3 3 0 1 1-6 0 3 3 0 0 1 6 0",
  organize: "M9 3h6v5H9V3ZM3 16h6v5H3v-5Zm12 0h6v5h-6v-5ZM12 8v4M6 16v-4h12v4",
  chevron: "m9 5 7 7-7 7",
  down: "m6 9 6 6 6-6",
  back: "m12 5-7 7 7 7M5 12h15",
  close: "m6 6 12 12M6 18 18 6",
  game: "M6.5 7h11c1.4 0 2.5 1 2.8 2.3l1.2 5.8c.5 2.5-2.4 4.1-4.1 2.3L15.1 15H8.9l-2.3 2.4c-1.7 1.8-4.6.2-4.1-2.3l1.2-5.8C4 8 5.1 7 6.5 7ZM7 10v4m-2-2h4m6-1h.5m1.5 2h.5",
  tag: "M3 3h8l10 10-8 8L3 11V3Zm4 4h.01",
  branch:
    "M8 5a2 2 0 1 1-4 0 2 2 0 0 1 4 0Zm0 14a2 2 0 1 1-4 0 2 2 0 0 1 4 0ZM20 5a2 2 0 1 1-4 0 2 2 0 0 1 4 0ZM6 7v10M18 7v2a5 5 0 0 1-5 5H6",
  diff: "M5 3h10l4 4v14H5V3Zm10 0v5h4M8 12h6m-3-3v6m-3 3h6",
  history: "M3 11a9 9 0 1 1 2.6 7.4M3 4v7h7m2-4v5l4 2",
  file: "M5 3h9l5 5v13H5V3Zm9 0v6h5M8 13h8M8 17h5",
  check: "m5 12 4 4L19 6",
  copy: "M8 8h12v13H8zM16 8V3H3v13h5",
  open: "M14 3h7v7m0-7L10 14M10 3H3v18h18v-7",
  refresh: "M20 7A9 9 0 0 0 4 6L2 9m0-6v6h6m-4 8a9 9 0 0 0 16 1l2-3m0 6v-6h-6",
  warning: "m12 3 10 18H2L12 3Zm0 6v5m0 3h.01",
  help: "M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0ZM9.4 9a2.7 2.7 0 0 1 5.2 1c0 1.8-2.6 2.1-2.6 4m0 3h.01",
  play: "m8 4 12 8-12 8V4Z",
  pause: "M7 4v16M17 4v16",
  focus: "M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5M8 12h8m-4-4v8",
  save: "M4 3h13l4 4v14H3V3h1Zm3 0v6h10V3M7 21v-8h10v8",
  archive: "M3 3h18v5H3zM5 8v13h14V8M9 12h6",
  clock: "M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-5v5l3 2",
  database:
    "M21 5c0 1.7-4 3-9 3S3 6.7 3 5s4-3 9-3 9 1.3 9 3ZM3 5v14c0 1.7 4 3 9 3s9-1.3 9-3V5M3 12c0 1.7 4 3 9 3s9-1.3 9-3",
  mesh: "M12 3 3 8v9l9 5 9-5V8l-9-5Zm0 0v19M3 8l18 9M21 8 3 17M3 8l9 5 9-5",
  sparkle: "m12 2 3 7 7 3-7 3-3 7-3-7-7-3 7-3 3-7Z",
  arrow: "M5 12h14m-6-6 6 6-6 6",
} as const;

export type IconName = keyof typeof paths;
export function Icon({
  name,
  size = 18,
  className,
  style,
}: {
  name: IconName;
  size?: number;
  className?: string;
  style?: CSSProperties;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className={className}
      style={style}
    >
      <path d={paths[name]} />
    </svg>
  );
}
