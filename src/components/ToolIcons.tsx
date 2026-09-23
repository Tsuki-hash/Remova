import type { ReactNode } from "react";

/** Unified 18px stroke glyphs for toolbox cards (replaces mixed emoji/text). */
export type ToolIconName =
  | "history"
  | "restore"
  | "whitelist"
  | "idle"
  | "installers"
  | "disk"
  | "toolcache"
  | "monitor"
  | "ai"
  | "update"
  | "report"
  | "orphan"
  | "force";

const PATHS: Record<ToolIconName, ReactNode> = {
  history: (
    <>
      <circle cx="12" cy="12" r="8" />
      <path d="M12 8v4l2.5 1.5" />
    </>
  ),
  restore: (
    <>
      <path d="M4 10a8 8 0 1 1 2.3 6.3" />
      <path d="M4 5v5h5" />
    </>
  ),
  whitelist: (
    <>
      <circle cx="12" cy="12" r="8" />
      <path d="M7.5 16.5 16.5 7.5" />
    </>
  ),
  idle: (
    <>
      <circle cx="12" cy="12" r="8" />
      <path d="M12 8v4l-2.5 1.5" />
      <path d="M5 5 8 3" />
    </>
  ),
  installers: (
    <>
      <path d="M12 4v10" />
      <path d="M8 11l4 4 4-4" />
      <path d="M5 18h14" />
    </>
  ),
  disk: (
    <>
      <path d="M5 7v10" />
      <path d="M10 10v7" />
      <path d="M15 5v12" />
      <path d="M20 9v8" />
    </>
  ),
  toolcache: (
    <>
      <rect x="4" y="5" width="16" height="14" rx="2" />
      <path d="M8 10h3" />
      <path d="M8 14h5" />
    </>
  ),
  monitor: (
    <>
      <circle cx="11" cy="11" r="6" />
      <path d="M16 16l3.5 3.5" />
    </>
  ),
  ai: (
    <>
      <path d="M12 4l1.4 4.2L18 10l-4.6 1.8L12 16l-1.4-4.2L6 10l4.6-1.8L12 4z" />
    </>
  ),
  update: (
    <>
      <path d="M12 19V6" />
      <path d="M7 11l5-5 5 5" />
    </>
  ),
  report: (
    <>
      <path d="M7 4h7l4 4v12H7z" />
      <path d="M14 4v4h4" />
    </>
  ),
  orphan: (
    <>
      <circle cx="12" cy="12" r="7" strokeDasharray="3 2.5" />
    </>
  ),
  force: (
    <>
      <path d="M8 8l8 8" />
      <path d="M16 8l-8 8" />
      <circle cx="12" cy="12" r="8" />
    </>
  ),
};

export function ToolGlyph({ name }: { name: ToolIconName }) {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      {PATHS[name]}
    </svg>
  );
}
