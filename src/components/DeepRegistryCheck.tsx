import { useState } from "react";
import { api } from "../lib/api";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import type { RegistryCheckResult } from "../types";

/** M5-4: explicit, read-only deep key search. Never offers a delete control. */
export function DeepRegistryCheck({ appName }: { appName: string }) {
  const L = t();
  const [result, setResult] = useState<RegistryCheckResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      setResult(await api.deepRegistryCheck(appName));
    } catch (e) {
      setError(formatError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={{ marginTop: 10, fontSize: 12 }}>
      <button
        style={{ ...css.btnGhost, height: 30 }}
        disabled={busy}
        onClick={() => void run()}
      >
        {busy ? L.deepRegistryChecking : L.deepRegistryCheck}
      </button>
      <span style={{ ...css.muted, marginLeft: 8 }}>{L.deepRegistryReadOnly}</span>
      {error && (
        <div role="alert" style={{ marginTop: 6, color: "var(--danger-text)" }}>
          {L.deepRegistryFailed(error)}
        </div>
      )}
      {result && (
        <div style={{ marginTop: 6 }}>
          <div style={{ fontWeight: 600 }}>{L.deepRegistryHits(result.hits.length)}</div>
          {result.truncated && <div style={css.muted}>{L.deepRegistryTruncated}</div>}
          {result.timed_out && <div style={css.muted}>{L.deepRegistryTimedOut}</div>}
          {result.hits.length === 0 ? (
            <div style={css.muted}>{L.deepRegistryEmpty}</div>
          ) : (
            <ul style={{ margin: "4px 0 0", paddingLeft: 18 }}>
              {result.hits.map((hit) => (
                <li key={hit.key} style={{ overflowWrap: "anywhere" }}>
                  <code>{hit.key}</code>
                  <span style={css.muted}> · {hit.detail}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
