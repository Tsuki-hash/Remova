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
  | "force"
  | "folder"
  | "file"
  | "startup"
  | "services"
  | "tasks";

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
  folder: (
    <>
      <path d="M3 7.5A1.5 1.5 0 0 1 4.5 6h4.2l1.8 2H19.5A1.5 1.5 0 0 1 21 9.5v8A1.5 1.5 0 0 1 19.5 19h-15A1.5 1.5 0 0 1 3 17.5v-10z" />
    </>
  ),
  file: (
    <>
      <path d="M7 3.5h6.5L19 9v11.5H7z" />
      <path d="M13.5 3.5V9H19" />
    </>
  ),
  startup: (
    <>
      <path d="M13 3L6 13h5l-1 8 8-12h-5l1-6z" />
    </>
  ),
  services: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M12 3.5v2.2M12 18.3v2.2M3.5 12h2.2M18.3 12h2.2M6 6l1.6 1.6M16.4 16.4 18 18M18 6l-1.6 1.6M7.6 16.4 6 18" />
    </>
  ),
  tasks: (
    <>
      <rect x="4" y="5" width="16" height="15" rx="2" />
      <path d="M8 3v4M16 3v4M4 10h16" />
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
