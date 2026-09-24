import { useEffect, useState, type CSSProperties, type ReactNode } from "react";
import { CloseGlyph } from "./ui/Glyph";
import { api } from "../lib/api";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { toast } from "../lib/toast";
import type { AiConfigView } from "../types";

const empty: AiConfigView = {
  enabled: false,
  provider: "openai",
  base_url: "https://api.openai.com/v1",
  model: "gpt-4o-mini",
  allow_cloud_paths: false,
  has_api_key: false,
};

type Preset = {
  id: string;
  labelKey: "aiProviderOpenAI" | "aiProviderAnthropic" | "aiProviderOllama";
  base: string;
  model: string;
  needsKey: boolean;
};

const PRESETS: Preset[] = [
  {
    id: "openai",
    labelKey: "aiProviderOpenAI",
    base: "https://api.openai.com/v1",
    model: "gpt-4o-mini",
    needsKey: true,
  },
  {
    id: "anthropic",
    labelKey: "aiProviderAnthropic",
    base: "https://api.anthropic.com/v1",
    model: "claude-sonnet-4-5",
    needsKey: true,
  },
  {
    id: "ollama",
    labelKey: "aiProviderOllama",
    base: "http://127.0.0.1:11434/v1",
    model: "llama3.2",
    needsKey: false,
  },
];

function Label({ children, hint }: { children: string; hint?: ReactNode }) {
  return (
    <div style={{ marginBottom: 6, display: "flex", alignItems: "baseline", gap: 8 }}>
      <span style={{ fontSize: 12, fontWeight: 650, color: "var(--fg)" }}>{children}</span>
      {hint && <span style={{ fontSize: 11.5, color: "var(--muted)" }}>{hint}</span>}
    </div>
  );
}

const inputStyle: CSSProperties = {
  ...css.input,
  height: 36,
  width: "100%",
  maxWidth: "none",
};

export function AiSettingsPanel({ onClose }: { onClose: () => void }) {
  const L = t();
  const [cfg, setCfg] = useState<AiConfigView>(empty);
  const [apiKey, setApiKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [dirtyPreset, setDirtyPreset] = useState(false);

  useEffect(() => {
    void api.getAiConfig()
      .then(setCfg)
      .catch(() => {});
  }, []);

  const applyPreset = (p: Preset) => {
    setCfg((c) => ({
      ...c,
      provider: p.id,
      base_url: p.base,
      model: p.model,
    }));
    setDirtyPreset(false);
  };

  const save = async () => {
    setBusy(true);
    try {
      const next = await api.saveAiConfig({
        enabled: cfg.enabled,
        provider: cfg.provider,
        baseUrl: cfg.base_url,
        model: cfg.model,
        allowCloudPaths: cfg.allow_cloud_paths,
        apiKey: apiKey.trim() || null,
      });
      setCfg(next);
      setApiKey("");
      toast.success(L.aiSaved);
    } catch (e) {
      toast.error(formatError(e));
    } finally {
      setBusy(false);
    }
  };

  const activePreset = PRESETS.find((p) => p.id === cfg.provider) ?? PRESETS[0];

  return (
    <div
      style={{
        ...css.card,
        padding: 0,
        overflow: "hidden",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 12,
          padding: "14px 16px",
          borderBottom: "1px solid var(--border)",
          background: "var(--surface-2)",
        }}
      >
        <div style={{ minWidth: 0, flex: 1 }}>
          <div style={{ fontWeight: 700, fontSize: 14 }}>{L.aiSettings}</div>
          <div style={{ ...css.muted, marginTop: 2 }}>{L.aiSettingsHint}</div>
        </div>
        <button style={{ ...css.btnSm, width: 30 }} onClick={onClose} aria-label="close">
          <CloseGlyph />
        </button>
      </div>

      <div style={{ padding: "14px 16px 16px", display: "flex", flexDirection: "column", gap: 14 }}>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            padding: "10px 12px",
            borderRadius: 10,
            background: cfg.enabled ? "var(--accent-soft)" : "var(--surface-2)",
            border: "1px solid var(--border)",
          }}
        >
          <input
            type="checkbox"
            checked={cfg.enabled}
            onChange={(e) => setCfg((c) => ({ ...c, enabled: e.target.checked }))}
            style={{ accentColor: "var(--accent)", width: 16, height: 16 }}
          />
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ fontWeight: 650, fontSize: 13 }}>{L.aiEnable}</div>
            <div style={{ ...css.muted, fontSize: 12 }}>{L.aiNeverDelete}</div>
          </div>
          <span
            style={{
              ...css.chip,
              color: cfg.enabled ? "var(--accent)" : "var(--muted)",
              background: cfg.enabled ? "var(--surface)" : "var(--surface-2)",
            }}
          >
            {cfg.enabled ? "ON" : "OFF"}
          </span>
        </div>

        <div>
          <Label hint={L.aiPresetHint}>{L.aiProvider}</Label>
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            {PRESETS.map((p) => {
              const active = cfg.provider === p.id;
              return (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => applyPreset(p)}
                  style={{
                    ...css.btnSm,
                    height: 34,
                    borderRadius: 8,
                    fontWeight: active ? 650 : 500,
                    borderColor: active ? "var(--accent)" : "var(--border)",
                    color: active ? "var(--accent)" : "var(--fg)",
                    background: active ? "var(--accent-soft)" : "var(--surface)",
                  }}
                >
                  {t()[p.labelKey]}
                </button>
              );
            })}
          </div>
        </div>

        <div style={{ display: "grid", gridTemplateColumns: "1.4fr 1fr", gap: 12 }}>
          <div style={{ minWidth: 0 }}>
            <Label>{L.aiBaseUrl}</Label>
            <input
              style={inputStyle}
              value={cfg.base_url}
              onChange={(e) => {
                setDirtyPreset(true);
                setCfg((c) => ({ ...c, base_url: e.target.value }));
              }}
              spellCheck={false}
              autoComplete="off"
            />
          </div>
          <div style={{ minWidth: 0 }}>
            <Label hint={dirtyPreset ? L.aiCustomPreset : undefined}>{L.aiModel}</Label>
            <input
              style={inputStyle}
              value={cfg.model}
              onChange={(e) => {
                setDirtyPreset(true);
                setCfg((c) => ({ ...c, model: e.target.value }));
              }}
              spellCheck={false}
              autoComplete="off"
            />
          </div>
        </div>

        <div>
          <Label
            hint={
              cfg.has_api_key ? (
                <span style={{ color: "var(--ok)" }}>{L.aiApiKeySet}</span>
              ) : activePreset.needsKey ? (
                <span style={{ color: "var(--warn)" }}>{L.aiApiKeyMissing}</span>
              ) : undefined
            }
          >
            {L.aiApiKey}
          </Label>
          <input
            style={inputStyle}
            type="password"
            placeholder={cfg.has_api_key ? L.aiApiKeyKeep : activePreset.needsKey ? "sk-…" : "—"}
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            autoComplete="new-password"
            disabled={cfg.provider === "ollama" && !cfg.base_url.includes("api.")}
          />
        </div>

        <label
          style={{
            display: "flex",
            gap: 10,
            alignItems: "flex-start",
            fontSize: 12.5,
            color: "var(--muted)",
            lineHeight: 1.45,
            cursor: "pointer",
          }}
        >
          <input
            type="checkbox"
            checked={cfg.allow_cloud_paths}
            onChange={(e) => setCfg((c) => ({ ...c, allow_cloud_paths: e.target.checked }))}
            style={{ marginTop: 2, accentColor: "var(--accent)" }}
          />
          <span>
            <span style={{ color: "var(--fg)", fontWeight: 600 }}>{L.aiAllowPaths}</span>
            <br />
            {L.aiPrivacyNote}
          </span>
        </label>

        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <button style={css.btn} disabled={busy} onClick={() => void save()}>
            {busy ? "…" : L.aiSave}
          </button>
          <span style={css.muted}>
            {cfg.provider} · {cfg.model || "—"}
          </span>
        </div>
      </div>
    </div>
  );
}
