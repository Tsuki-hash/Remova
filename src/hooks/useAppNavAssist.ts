import { useEffect, useRef, type RefObject } from "react";
import { api } from "../lib/api";
import { getConfirm, subscribeConfirm } from "../lib/confirm";
import type { InstalledApp } from "../types";
import { toast } from "../lib/toast";
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

/** Consume initial and forwarded --analyze requests from the same queue. */
export function usePendingAnalyze({
  blocked,
  busyRef,
  apps,
  analyze,
  setQ,
}: {
  blocked: boolean;
  busyRef: RefObject<boolean>;
  apps: InstalledApp[];
  analyze: (a: InstalledApp) => Promise<boolean>;
  setQ: (q: string) => void;
}) {
  const latest = useRef({ blocked, apps, analyze, setQ });
  const wake = useRef<() => void>(() => {});
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let draining = false;
    let notified = false;
    let next: string | null = null;
    let resumeAfterConfirm: ReturnType<typeof setTimeout> | undefined;
    const canDrain = () =>
      !disposed && !latest.current.blocked && !busyRef.current &&
      resumeAfterConfirm === undefined && getConfirm() === null;

    const drain = async () => {
      notified = true;
      if (draining || !canDrain()) return;
      draining = true;
      try {
        do {
          notified = false;
          while (canDrain()) {
            next ??= await api.takePendingAnalyze();
            // A confirmation may open while the native queue is returning a path.
            if (!next || !canDrain()) break;
            const { apps, analyze, setQ } = latest.current;
            const path = next;
            next = null;
            const app = appForPath(apps, path);
            if (app) await analyze(app);
            else {
              setQ(path);
              toast.info(path);
            }
          }
        } while (notified && canDrain());
      } catch {
        // This entry point is inactive outside the native app.
      } finally {
        draining = false;
      }
    };
    wake.current = () => { void drain(); };
    const unsubscribeConfirm = subscribeConfirm(() => {
      clearTimeout(resumeAfterConfirm);
      resumeAfterConfirm = undefined;
      if (getConfirm()) return;
      // Give the confirmation handler a turn to start its operation first.
      resumeAfterConfirm = setTimeout(() => {
        resumeAfterConfirm = undefined;
        void drain();
      }, 0);
    });
    void import("@tauri-apps/api/event")
      .then(({ listen }) => listen("pending-analyze", () => { void drain(); }))
      .then(stop => {
        if (disposed) stop();
        else {
          unlisten = stop;
          void drain();
        }
      })
      .catch(() => {});
    return () => {
      disposed = true;
      clearTimeout(resumeAfterConfirm);
      unsubscribeConfirm();
      unlisten?.();
    };
  }, [busyRef]);
  useEffect(() => {
    latest.current = { blocked, apps, analyze, setQ };
  }, [blocked, apps, analyze, setQ]);
  useEffect(() => {
    if (!blocked) wake.current();
  }, [blocked]);
}

/** Drag-drop: match dropped path to install_location and analyze. */
export function useDragDropAnalyze({
  apps,
  analyze,
  onUnmatched,
  preferFolder = false,
}: {
  apps: InstalledApp[];
  analyze: (a: InstalledApp) => void;
  onUnmatched?: (path: string) => void;
  preferFolder?: boolean;
}) {
 // read the latest inputs through a ref so the webview channel is subscribed once
 // instead of being torn down and rebuilt every time the app list changes.
  const latest = useRef({ apps, analyze, onUnmatched, preferFolder });
  useEffect(() => {
    latest.current = { apps, analyze, onUnmatched, preferFolder };
  }, [apps, analyze, onUnmatched, preferFolder]);

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
          const { apps, analyze, onUnmatched, preferFolder } = latest.current;
          const hit = preferFolder ? null : appForPath(apps, path);
          if (hit) {
            analyze(hit);
          } else if (onUnmatched) {
            onUnmatched(path);
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
