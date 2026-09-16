import { useEffect, useState } from "react";
import { api } from "../lib/api";

/** Process-wide icon cache: displayIcon raw → data URL (or null on failure). */
const iconCache = new Map<string, string | null>();
const iconInflight = new Map<string, Promise<string | null>>();

function loadAppIcon(displayIcon: string): Promise<string | null> {
  if (iconCache.has(displayIcon)) {
    return Promise.resolve(iconCache.get(displayIcon) ?? null);
  }
  const existing = iconInflight.get(displayIcon);
  if (existing) return existing;
  const p = api.appIconData(displayIcon)
    .then((url) => {
      iconCache.set(displayIcon, url);
      iconInflight.delete(displayIcon);
      return url;
    })
    .catch(() => {
      iconCache.set(displayIcon, null);
      iconInflight.delete(displayIcon);
      return null;
    });
  iconInflight.set(displayIcon, p);
  return p;
}

export function AppIcon({ displayIcon, name }: { displayIcon: string; name: string }) {
  const [src, setSrc] = useState<string | null>(() =>
    displayIcon ? (iconCache.get(displayIcon) ?? null) : null,
  );

  useEffect(() => {
    if (!displayIcon) {
      setSrc(null);
      return;
    }
    let cancelled = false;
    void loadAppIcon(displayIcon).then((url) => {
      if (!cancelled) setSrc(url);
    });
    return () => {
      cancelled = true;
    };
  }, [displayIcon]);

  const initial = (name.trim()[0] || "?").toUpperCase();
  return (
    <span
      style={{
        width: 32,
        height: 32,
        borderRadius: 8,
        flexShrink: 0,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        background: "var(--surface-2)",
        color: "var(--muted)",
        fontSize: 13,
        fontWeight: 700,
        overflow: "hidden",
      }}
      aria-hidden
    >
      {src ? (
        <img src={src} width={32} height={32} alt="" style={{ display: "block" }} />
      ) : (
        initial
      )}
    </span>
  );
}
