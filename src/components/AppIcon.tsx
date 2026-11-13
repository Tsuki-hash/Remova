import { useEffect, useState, type ReactNode } from "react";
import { api } from "../lib/api";

/** Share loaded icons between the virtualized list and the detail pane. */
const iconCache = new Map<string, string>();
const iconInflight = new Map<string, Promise<string | null>>();
const MAX_CACHE_CHARS = 8 * 1024 * 1024;
let cachedChars = 0;

function forgetIcon(key: string) {
  cachedChars -= iconCache.get(key)?.length ?? 0;
  iconCache.delete(key);
}

function loadAppIcon(displayIcon: string, embeddedOnly: boolean, key: string): Promise<string | null> {
  const cached = iconCache.get(key);
  if (cached) {
    iconCache.delete(key);
    iconCache.set(key, cached);
    return Promise.resolve(cached);
  }
  const existing = iconInflight.get(key);
  if (existing) return existing;
  const p = api.appIconData(displayIcon, embeddedOnly)
    .then((url) => {
      if (url) {
        iconCache.set(key, url);
        cachedChars += url.length;
        for (const oldest of iconCache.keys()) {
          if (cachedChars <= MAX_CACHE_CHARS) break;
          forgetIcon(oldest);
        }
      }
      iconInflight.delete(key);
      return url;
    })
    .catch(() => {
      iconInflight.delete(key);
      return null;
    });
  iconInflight.set(key, p);
  return p;
}

export function AppIcon({ displayIcon, name, fallback, embeddedOnly = false, bare = false }: {
  displayIcon: string;
  name: string;
  fallback?: ReactNode;
  embeddedOnly?: boolean;
  bare?: boolean;
}) {
  const key = embeddedOnly ? "embedded\0" + displayIcon : displayIcon;
  const [loaded, setLoaded] = useState<{ key: string; url: string | null } | null>(null);
  const [failedImage, setFailedImage] = useState<{ key: string; url: string } | null>(null);
  const src = loaded?.key === key ? loaded.url : iconCache.get(key);
  const imageFailed = failedImage?.key === key && failedImage.url === src;

  useEffect(() => {
    if (!displayIcon) return;
    let cancelled = false;
    void loadAppIcon(displayIcon, embeddedOnly, key).then((url) => {
      if (!cancelled) setLoaded({ key, url });
    });
    return () => {
      cancelled = true;
    };
  }, [displayIcon, embeddedOnly, key]);

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
        background: bare ? "transparent" : "var(--surface-2)",
        color: bare ? "var(--accent-text)" : "var(--muted)",
        fontSize: 13,
        fontWeight: 700,
        overflow: "hidden",
      }}
      aria-hidden
    >
      {src && !imageFailed ? (
        <img src={src} width={32} height={32} alt="" style={{ display: "block", objectFit: "contain" }}
          onError={() => {
            forgetIcon(key);
            setFailedImage({ key, url: src });
          }} />
      ) : (
        fallback ?? initial
      )}
    </span>
  );
}
