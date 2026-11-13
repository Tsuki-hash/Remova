import { useEffect, useState, type ReactNode } from "react";
import { api } from "../lib/api";
import { appKey } from "../lib/appKey";
import type { InstalledApp } from "../types";
import { ToolGlyph } from "./ToolIcons";

/** Share loaded icons between the virtualized list and the detail pane. */
const iconCache = new Map<string, string>();
const iconInflight = new Map<string, Promise<string | null>>();
const MAX_CACHE_CHARS = 8 * 1024 * 1024;
let cachedChars = 0;

function forgetIcon(key: string) {
  cachedChars -= iconCache.get(key)?.length ?? 0;
  iconCache.delete(key);
}

function loadAppIcon(displayIcon: string, embeddedOnly: boolean, key: string, installedAppKey?: string): Promise<string | null> {
  const cached = iconCache.get(key);
  if (cached) {
    iconCache.delete(key);
    iconCache.set(key, cached);
    return Promise.resolve(cached);
  }
  const existing = iconInflight.get(key);
  if (existing) return existing;
  const request = installedAppKey
    ? api.appIconData(displayIcon, embeddedOnly, installedAppKey)
    : api.appIconData(displayIcon, embeddedOnly);
  const p = request
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

export function InstalledAppIcon({ app }: { app: InstalledApp }) {
  return <AppIcon displayIcon={app.display_icon} name={app.name} installedAppKey={appKey(app)} version={app.version}
    bare fallback={<ToolGlyph name="package" size={19} />} />;
}

export function AppIcon({ displayIcon, name, fallback, embeddedOnly = false, bare = false, installedAppKey, version }: {
  displayIcon: string;
  name: string;
  fallback?: ReactNode;
  embeddedOnly?: boolean;
  bare?: boolean;
  installedAppKey?: string;
  version?: string;
}) {
  const key = installedAppKey ? `installed\0${installedAppKey}\0${version}\0${displayIcon}`
    : embeddedOnly ? "embedded\0" + displayIcon : displayIcon;
  const [loaded, setLoaded] = useState<{ key: string; url: string | null } | null>(null);
  const [failedImage, setFailedImage] = useState<{ key: string; url: string } | null>(null);
  const src = loaded?.key === key ? loaded.url : iconCache.get(key);
  const imageFailed = failedImage?.key === key && failedImage.url === src;

  useEffect(() => {
    if (!displayIcon && !installedAppKey) return;
    let cancelled = false;
    void loadAppIcon(displayIcon, embeddedOnly, key, installedAppKey).then((url) => {
      if (!cancelled) setLoaded({ key, url });
    });
    return () => {
      cancelled = true;
    };
  }, [displayIcon, embeddedOnly, installedAppKey, key]);

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
        color: bare && !installedAppKey ? "var(--accent-text)" : "var(--muted)",
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
