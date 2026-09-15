import { useEffect, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
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

export function AiSettingsPanel({ onClose }: { onClose: () => void }) {
  const L = t();
  const [cfg, setCfg] = useState<AiConfigView>(empty);
  const [apiKey, setApiKey] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void invoke<AiConfigView>("get_ai_config")
      .then(setCfg)
      .catch(() => {});
  }, []);

  const save = async () => {
    setBusy(true);
    try {
      const next = await invoke<AiConfigView>("save_ai_config", {
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
      toast.error(L.errInvokeFailed(formatError(e)));
    } finally {
      setBusy(false);
    }
  };

  const field: CSSProperties = {
    ...css.input,
    flex: "1 1 220px",
    height: 34,
    maxWidth: 360,
  };

  return (
    <div style={{ ...css.card, padding: 14, display: "flex", flexDirection: "column", gap: 12 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <strong>{L.aiSettings}</strong>
        <span style={css.muted}>{L.aiSettingsHint}</span>
        <button style={{ ...css.btnSm, marginLeft: "auto" }} onClick={onClose}>
          ×
        </button>
      </div>
      <label style={{ display: "flex", gap: 8, alignItems: "center", fontSize: 13 }}>
        <input
          type="checkbox"
          checked={cfg.enabled}
          onChange={(e) => setCfg((c) => ({ ...c, enabled: e.target.checked }))}
        />
        {L.aiEnable}
      </label>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center" }}>
        <select
          style={{ ...field, flex: "0 0 auto", minWidth: 160 }}
          value={cfg.provider}
          onChange={(e) => {
            const provider = e.target.value;
            setCfg((c) => ({
              ...c,
              provider,
              base_url:
                provider === "ollama"
                  ? "http://127.0.0.1:11434/v1"
                  : c.base_url || "https://api.openai.com/v1",
              model: provider === "ollama" && !c.model ? "llama3.2" : c.model,
            }));
          }}
        >
          <option value="openai">{L.aiProviderOpenAI}</option>
          <option value="ollama">{L.aiProviderOllama}</option>
        </select>
        <input
          style={field}
          placeholder={L.aiBaseUrl}
          value={cfg.base_url}
          onChange={(e) => setCfg((c) => ({ ...c, base_url: e.target.value }))}
        />
        <input
          style={{ ...field, flex: "0 1 160px" }}
          placeholder={L.aiModel}
          value={cfg.model}
          onChange={(e) => setCfg((c) => ({ ...c, model: e.target.value }))}
        />
      </div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center" }}>
        <input
          style={field}
          type="password"
          placeholder={
            cfg.has_api_key ? `${L.aiApiKey} · ${L.aiApiKeyKeep}` : L.aiApiKey
          }
          value={apiKey}
          onChange={(e) => setApiKey(e.target.value)}
          autoComplete="off"
        />
      </div>
      <label style={{ display: "flex", gap: 8, alignItems: "center", fontSize: 13, color: "var(--muted)" }}>
        <input
          type="checkbox"
          checked={cfg.allow_cloud_paths}
          onChange={(e) => setCfg((c) => ({ ...c, allow_cloud_paths: e.target.checked }))}
        />
        {L.aiAllowPaths}
      </label>
      <div>
        <button style={css.btn} disabled={busy} onClick={() => void save()}>
          {L.aiSave}
        </button>
      </div>
    </div>
  );
}
