import type { ReactNode } from "react";

/** Shared stroke glyphs for navigation and maintenance tools. */
export type ToolIconName =
  | "history"
  | "restore"
  | "whitelist"
  | "idle"
  | "installers"
  | "disk"
  | "drive"
  | "cleanup"
  | "broken-link"
  | "migrate"
  | "toolbox"
  | "package"
  | "task-multiple"
  | "target"
  | "settings"
  | "apps"
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
  | "system-service"
  | "task-command"
  | "tasks";

const PATHS: Record<ToolIconName, ReactNode> = {
  apps: <path d="M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z" strokeLinejoin="round" />,
  settings: <path d="M4 6h16M4 12h16M4 18h16M8 4v4m8 2v4m-5 2v4" />,
  drive: <path d="M5 6h14l2 9v3H3v-3l2-9ZM3 15h18m-4 0v.1" />,
  cleanup: <path d="m15 3 2 1-4 8m-3-2 5 3-4 8-8-4 3-5 4-2Zm7 7h4m-2-2v4" />,
  "broken-link": <path d="m9 15-1 1a3 3 0 0 1-4-4l3-3a3 3 0 0 1 4 0m2 0 1-1a3 3 0 0 1 4 4l-3 3a3 3 0 0 1-4 0m7-11 2-2m-2 5h3m-7-4V1" />,
  migrate: <path d="M21 10V8H11l-2-3H3v14h9m1-4h8m-3-3 3 3-3 3" />,
  toolbox: <path d="M4 7h16l1 3v9H3v-9l1-3Zm5 0V4h6v3M3 12h18m-12 0v2m6-2v2" />,
  package: <path d="m12 3 9 5v8l-9 5-9-5V8l9-5ZM3 8l9 5 9-5m-9 5v8M7.5 5.5l9 5" />,
  "task-multiple": <path d="M5 3h14v17H5zM8 7h2m3 0h3M8 11h2m3 0h3M8 15h2m3 0h3" />,
  target: <path d="M12 3v4m0 10v4M3 12h4m10 0h4m-2 0a7 7 0 1 0-14 0 7 7 0 0 0 14 0Z" />,
  "task-command": (
    <>
      <rect x="3" y="5" width="18" height="14" rx="2" />
      <path d="m7 9 3 3-3 3m6 0h4" />
    </>
  ),
  "system-service": (
    <>
      <rect x="4" y="4" width="16" height="16" rx="2" />
      <path d="M12 4v16M4 12h16" />
    </>
  ),
  history: (
    <>
      <circle cx="12" cy="12" r="8" />
      <path d="M12 8v4l2.5 1.5" />
    </>
  ),
  restore: (
    <>
      <path d="M3 10h18V7l-3-3H6L3 7v3Zm2 0v10h6m8-10v3m-5 3h4a3 3 0 0 1 3 3v1m-5-7-3 3 3 3" />
    </>
  ),
  whitelist: (
    <>
      <path d="M12 3 4 6v5c0 5 3 8 8 10 5-2 8-5 8-10V6l-8-3Zm-3 9 2 2 4-4" />
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
      <ellipse cx="12" cy="6" rx="8" ry="3" />
      <path d="M4 6v12a8 3 0 0 0 16 0V6M4 12a8 3 0 0 0 16 0" />
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
      <path d="M9.8 3.5h4.4l.6 2.5 1.1.6 2.5-.7 2.2 3.8-1.8 1.8v1.2l1.8 1.8-2.2 3.8-2.5-.7-1.1.6-.6 2.5H9.8l-.6-2.5-1.1-.6-2.5.7-2.2-3.8 1.8-1.8v-1.2L3.4 9.7l2.2-3.8 2.5.7 1.1-.6.6-2.5ZM15 12a3 3 0 1 0-6 0 3 3 0 0 0 6 0Z" fillRule="evenodd" />
    </>
  ),
  tasks: (
    <>
      <path d="M12 20H6a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v4M8 3v4m8-4v4M4 10h16" fill="none" />
      <circle cx="17.5" cy="17.5" r="4.2" fill="none" />
      <path d="M17.5 15.3v2.4l1.4 1" />
    </>
  ),
};

export function ToolGlyph({ name, size = 18, filled = false }: { name: ToolIconName; size?: number; filled?: boolean }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill={filled ? "currentColor" : "none"}
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
