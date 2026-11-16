import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { api } from "../lib/api";
import { appKey } from "../lib/appKey";
import type { CleanupItem, InstalledApp, ManageItem } from "../types";
import { ToolGlyph, type ToolIconName } from "./ToolIcons";

/** Share loaded icons between lists and detail panes. */
const iconCache = new Map<string, string>();
const iconMisses = new Map<string, number>();
type IconListener = (url: string | null) => void;
type IconRequest = { read: () => Promise<string | null>; started: boolean; listeners: Set<IconListener> };
const iconRequests = new Map<string, IconRequest>();
const MAX_CACHE_CHARS = 8 * 1024 * 1024;
const MAX_ICON_REQUESTS = 4;
const MAX_MISSES = 512;
const MISS_TTL_MS = 30_000;
let cachedChars = 0;
let activeRequests = 0;

function forgetIcon(key: string) {
  cachedChars -= iconCache.get(key)?.length ?? 0;
  iconCache.delete(key);
}

function hasRecentMiss(key: string) {
  const expires = iconMisses.get(key);
  if (expires === undefined) return false;
  if (performance.now() < expires) return true;
  iconMisses.delete(key);
  return false;
}

function cacheIconResult(key: string, url: string | null) {
  if (url) {
    iconCache.set(key, url);
    cachedChars += url.length;
    for (const oldest of iconCache.keys()) {
      if (cachedChars <= MAX_CACHE_CHARS) break;
      forgetIcon(oldest);
    }
  } else if (url === null) {
    if (iconMisses.size === MAX_MISSES) {
      const oldest = iconMisses.keys().next().value;
      if (oldest !== undefined) iconMisses.delete(oldest);
    }
    iconMisses.set(key, performance.now() + MISS_TTL_MS);
  }
}

function startWaitingIcons() {
  for (const [key, request] of iconRequests) {
    if (activeRequests >= MAX_ICON_REQUESTS) break;
    if (request.started) continue;
    request.started = true;
    activeRequests++;
    const finish = (url: string | null) => {
      activeRequests--;
      iconRequests.delete(key);
      for (const listener of request.listeners) listener(url);
      startWaitingIcons();
    };
    void Promise.resolve().then(request.read).then(url => {
      cacheIconResult(key, url);
      finish(url);
    }, () => finish(null));
  }
}

type IconAppearance = { background: string; imageStyle?: CSSProperties };

function iconAppearance(img: HTMLImageElement): IconAppearance | null {
  // Packaged apps can return a white or black mark with transparent padding.
  // Sample the image without modifying the original logo colors.
  const sample = 48;
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = sample;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (!context) return null;
  context.drawImage(img, 0, 0, sample, sample);
  const { data } = context.getImageData(0, 0, sample, sample);
  let visible = 0;
  let light = 0;
  let dark = 0;
  let left = sample, top = sample, right = -1, bottom = -1;
  for (let offset = 0; offset < data.length; offset += 4) {
    if (data[offset + 3]! < 48) continue;
    visible++;
    const red = data[offset]!;
    const green = data[offset + 1]!;
    const blue = data[offset + 2]!;
    if (Math.min(red, green, blue) >= 218) light++;
    if (Math.max(red, green, blue) <= 48) dark++;
    const x = (offset / 4) % sample;
    const y = Math.floor(offset / 4 / sample);
    left = Math.min(left, x);
    right = Math.max(right, x);
    top = Math.min(top, y);
    bottom = Math.max(bottom, y);
  }
  if (!visible || visible > sample * sample * 0.8) return null;
  const background = light / visible > 0.9 ? "#263345" : dark / visible > 0.9 ? "#edf2f8" : null;
  if (!background) return null;
  const contentRatio = Math.max(right - left + 1, bottom - top + 1) / sample;
  if (contentRatio > 0.7) return { background };
  const x = ((left + right + 1) / 2 / sample) * 100;
  const y = ((top + bottom + 1) / 2 / sample) * 100;
  return {
    background,
    imageStyle: { transformOrigin: `${x}% ${y}%`, transform: `translate(${50 - x}%, ${50 - y}%) scale(${Math.min(2.4, 0.68 / contentRatio)})` },
  };
}

function watchAppIcon(key: string, read: IconRequest["read"], onReady: IconListener): () => void {
  const cached = iconCache.get(key);
  if (cached) {
    iconCache.delete(key);
    iconCache.set(key, cached);
  }
  if (cached || hasRecentMiss(key)) {
    queueMicrotask(() => onReady(cached ?? null));
    return () => {};
  }
  const request = iconRequests.get(key) ?? { read, started: false, listeners: new Set<IconListener>() };
  iconRequests.set(key, request);
  request.listeners.add(onReady);
  startWaitingIcons();
  return () => {
    request.listeners.delete(onReady);
    if (!request.started && request.listeners.size === 0) iconRequests.delete(key);
  };
}

export function InstalledAppIcon({ app }: { app: InstalledApp }) {
  return <AppIcon displayIcon={app.display_icon} installedAppKey={appKey(app)} version={app.version}
    bare fallback={<ToolGlyph name="package" size={19} />} />;
}

export function ManageItemIcon({ item, glyph }: { item: ManageItem; glyph: ToolIconName }) {
  return <AppIcon displayIcon={item.display_icon || ""} embeddedOnly bare
    relatedPath={!item.system_service && item.task_action?.kind !== "multiple" &&
      !(item.task_action?.kind === "program" && item.task_action.script_host) ? item.path || "" : ""}
    appUserModelId={item.app_user_model_id || ""} fallback={<ToolGlyph name={glyph} size={22} />} />;
}

export function LocalItemIcon({ path, kind = "file", size = 22 }: {
  path: string;
  kind?: CleanupItem["kind"];
  size?: number;
}) {
  const local = (kind === "file" || kind === "dir") && /^[a-z]:[\\/]/i.test(path);
  const fileIcon = local && kind === "file" && /\.(exe|dll|cpl|ico|msi)$/i.test(path) ? path : "";
  const glyph = kind === "dir" ? "folder" : kind === "registry" ? "system-service" : "file";
  return <AppIcon displayIcon={fileIcon} relatedPath={local ? path : ""} bare size={size} defer
    fallback={<ToolGlyph name={glyph} size={Math.min(20, size)} />} />;
}

export function AppIcon({ displayIcon, fallback, embeddedOnly = false, bare = false, installedAppKey,
  version, relatedPath = "", appUserModelId = "", size = 32, defer = false }: {
  displayIcon: string;
  fallback?: ReactNode;
  embeddedOnly?: boolean;
  bare?: boolean;
  installedAppKey?: string;
  version?: string;
  relatedPath?: string;
  appUserModelId?: string;
  size?: number;
  defer?: boolean;
}) {
  const key = installedAppKey ? `installed\0${installedAppKey}\0${version}\0${displayIcon}`
    : relatedPath || appUserModelId ? `related\0${embeddedOnly}\0${relatedPath}\0${appUserModelId}\0${displayIcon}`
    : embeddedOnly ? "embedded\0" + displayIcon : displayIcon;
  const [loaded, setLoaded] = useState<{ key: string; url: string | null } | null>(null);
  const [failedImage, setFailedImage] = useState<{ key: string; url: string } | null>(null);
  const [appearance, setAppearance] = useState<(IconAppearance & { src: string }) | null>(null);
  const element = useRef<HTMLSpanElement>(null);
  const src = loaded?.key === key ? loaded.url : iconCache.get(key);
  const imageFailed = failedImage?.key === key && failedImage.url === src;
  const activeAppearance = appearance?.src === src ? appearance : null;

  useEffect(() => {
    if (!displayIcon && !installedAppKey && !relatedPath && !appUserModelId) return;
    let cancelled = false;
    let stopWatching: (() => void) | undefined;
    const read = () => relatedPath || appUserModelId
      ? api.appIconData(displayIcon, embeddedOnly, undefined, { path: relatedPath, appUserModelId })
      : installedAppKey
        ? api.appIconData(displayIcon, embeddedOnly, installedAppKey)
        : api.appIconData(displayIcon, embeddedOnly);
    const load = () => {
      stopWatching ??= watchAppIcon(key, read, url => {
        if (!cancelled) setLoaded({ key, url });
      });
    };
    let observer: IntersectionObserver | undefined;
    if (defer && !iconCache.has(key) && !hasRecentMiss(key) && element.current && typeof IntersectionObserver !== "undefined") {
      observer = new IntersectionObserver((entries) => {
        if (cancelled || !entries.some((entry) => entry.isIntersecting)) return;
        observer?.disconnect();
        load();
      }, { rootMargin: "120px" });
      observer.observe(element.current);
    } else {
      load();
    }
    return () => {
      cancelled = true;
      observer?.disconnect();
      stopWatching?.();
    };
  }, [defer, displayIcon, embeddedOnly, installedAppKey, key, relatedPath, appUserModelId]);

  return (
    <span
      ref={element}
      style={{
        width: size,
        height: size,
        borderRadius: 8,
        flexShrink: 0,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        background: activeAppearance?.background ?? (bare ? "transparent" : "var(--surface-2)"),
        color: bare && !installedAppKey ? "var(--accent-text)" : "var(--muted)",
        fontSize: 13,
        fontWeight: 700,
        overflow: "hidden",
      }}
      aria-hidden
    >
      {src && !imageFailed ? (
        <img src={src} width={size} height={size} alt="" style={{ display: "block", objectFit: "contain", ...activeAppearance?.imageStyle }}
          onLoad={(event) => {
            const next = iconAppearance(event.currentTarget);
            if (next) setAppearance({ src, ...next });
          }}
          onError={() => {
            forgetIcon(key);
            setFailedImage({ key, url: src });
          }} />
      ) : (
        fallback ?? <ToolGlyph name="package" />
      )}
    </span>
  );
}
