import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import type { IdleApp } from "../types";

/** Idle software radar — read-only ranking; jump back to the software list to uninstall. */
export function IdleRadarPanel({
  onClose,
  onGoSoftware,
  onError,
}: {
  onClose: () => void;
  onGoSoftware: () => void;
  onError: (msg: string) => void;
}) {
  const L = t();
  const [rows, setRows] = useState<IdleApp[] | null>(null);
  const [busy, setBusy] = useState(false);

  const load = async () => {
    setBusy(true);
    try {
      setRows(await api.rankIdleApps());
    } catch (e) {
      onError(formatError(e));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <strong>{L.idleTitle}</strong>
        <span style={css.muted}>{L.idleHint}</span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          <button style={{ ...css.btnGhost, height: 30 }} disabled={busy} onClick={() => void load()}>
            {busy ? L.loadingApps : L.manageReload}
          </button>
          <button style={{ ...css.btnGhost, height: 30 }} onClick={onClose}>
            ×
          </button>
        </div>
      </div>
      <div style={{ maxHeight: 320, overflow: "auto", marginTop: 10 }}>
        {busy && !rows && <div style={css.muted}>{L.loadingApps}</div>}
        {rows && rows.length === 0 && <div style={css.muted}>{L.idleEmpty}</div>}
        {rows?.map((r) => (
          <div
            key={r.app.registry_key || r.app.name}
            style={{
              display: "flex",
              gap: 10,
              alignItems: "center",
              padding: "8px 10px",
              borderRadius: 8,
              border: "1px solid var(--border)",
              background: "var(--surface-2)",
              marginBottom: 6,
            }}
          >
            <div style={{ minWidth: 0, flex: 1 }}>
              <div style={{ fontWeight: 600 }}>{r.app.name}</div>
              <div style={{ ...css.muted, fontSize: 11.5, fontFamily: "var(--mono)" }}>
                {L.idleDays(r.idle_days)} · {formatSize(Math.max(0, r.size_kb))}
                {r.app.install_date ? ` · ${r.app.install_date}` : ""}
              </div>
              <div style={{ ...css.muted, fontSize: 11 }}>
                {r.evidence
                  .map((e) =>
                    e.code === "idle_install_age"
                      ? L.idleEvInstallAge
                      : e.code === "idle_dir_mtime"
                        ? L.idleEvDirMtime
                        : e.code,
                  )
                  .join(" · ")}
              </div>
            </div>
            <button style={{ ...css.btnGhost, height: 28 }} onClick={onGoSoftware}>
              {L.goToSoftware}
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
