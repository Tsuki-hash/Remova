import { useEffect, useRef } from "react";
import { api } from "../lib/api";
import type { InstalledApp } from "../types";
import { toast } from "../lib/toast";
import { prettyAppName } from "../lib/format";
import { t } from "../i18n";

/** Require a complete directory boundary and a unique most-specific owner. */
export function appForPath(apps: InstalledApp[], path: string): InstalledApp | null {
  const normalize = (value: string) => {
    const windows = value.replace(/\//g, "\\").toLowerCase();
    if (!/^(?:[a-z]:\\|\\\\[^\\]+\\[^\\]+)/.test(windows) ||
      windows.split("\\").some(part => part === "." || part === "..")) return "";
    return windows.replace(/\\+$/, "");
  };
  const target = normalize(path);
  if (!target) return null;
  const hits = apps.map(app => ({ app, root: normalize(app.install_location || "") }))
    .filter(({ root }) => root && (target === root || target.startsWith(`${root}\\`)))
    .sort((a, b) => b.root.length - a.root.length);
  if (!hits.length || (hits[1] && hits[0]!.root.length === hits[1].root.length)) return null;
  return hits[0]!.app;
}

/** Context menu --analyze: match pending path after list load. */
export function usePendingAnalyze({
  loading,
  apps,
  goNav,
  setSelected,
  analyze,
  setQ,
}: {
  loading: boolean;
  apps: InstalledApp[];
  goNav: (id: "software") => void;
  setSelected: (a: InstalledApp) => void;
  analyze: (a: InstalledApp) => void;
  setQ: (q: string) => void;
}) {
  useEffect(() => {
    if (loading || apps.length === 0) return;
    let cancelled = false;
    void api
      .takePendingAnalyze()
      .then((p) => {
        if (cancelled || !p) return;
        const hit = appForPath(apps, p);
        if (hit) {
          goNav("software");
          setSelected(hit);
          analyze(hit);
        } else {
          setQ(p);
          toast.info(p);
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
 // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loading, apps]);
}

/** Drag-drop: match dropped path to install_location and analyze. */
export function useDragDropAnalyze({
  apps,
  setSelected,
  analyze,
}: {
  apps: InstalledApp[];
  setSelected: (a: InstalledApp) => void;
  analyze: (a: InstalledApp) => void;
}) {
 // read the latest inputs through a ref so the webview channel is subscribed once
 // instead of being torn down and rebuilt every time the app list changes.
  const latest = useRef({ apps, setSelected, analyze });
  useEffect(() => {
    latest.current = { apps, setSelected, analyze };
  }, [apps, setSelected, analyze]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void (async () => {
      try {
        const { getCurrentWebview } = await import("@tauri-apps/api/webview");
        const webview = getCurrentWebview();
        const un = await webview.onDragDropEvent((event) => {
          if (event.payload.type !== "drop") return;
          const path = (event.payload.paths || [])[0];
          if (!path) return;
          const { apps, setSelected, analyze } = latest.current;
          const hit = appForPath(apps, path);
          if (hit) {
            setSelected(hit);
            analyze(hit);
            toast.info(prettyAppName(hit.name, hit.source));
          } else {
            toast.info(`${t().dropHint}: ${path}`);
          }
        });
        if (cancelled) un();
        else unlisten = un;
      } catch {
 // not in tauri
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}
