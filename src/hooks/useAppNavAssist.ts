import { useEffect } from "react";
import { api } from "../lib/api";
import type { InstalledApp } from "../types";
import { toast } from "../lib/toast";
import { prettyAppName } from "../lib/format";
import { t } from "../i18n";

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
        const low = p.toLowerCase();
        const hit = apps.find(
          (a) =>
            a.install_location &&
            low.startsWith(a.install_location.replace(/\//g, "\\").toLowerCase()),
        );
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
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      try {
        const { getCurrentWebview } = await import("@tauri-apps/api/webview");
        const webview = getCurrentWebview();
        const un = await webview.onDragDropEvent((event) => {
          if (event.payload.type !== "drop") return;
          const path = (event.payload.paths || [])[0];
          if (!path) return;
          const norm = path.replace(/\//g, "\\").toLowerCase();
          const hit =
            apps.find((a) => {
              const loc = (a.install_location || "").replace(/\//g, "\\").toLowerCase();
              return loc && (norm.startsWith(loc) || loc.startsWith(norm) || norm === loc);
            }) ?? null;
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [apps]);
}
