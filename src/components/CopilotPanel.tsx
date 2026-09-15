import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { toast } from "../lib/toast";
import type { InstalledApp, NlIntent } from "../types";

function applyFilter(apps: InstalledApp[], filter: NlIntent["filter"]): InstalledApp[] {
  let list = apps;
  const name = (filter.name_like || "").trim().toLowerCase();
  if (name) {
    list = list.filter((a) => a.name.toLowerCase().includes(name));
  }
  const pub = (filter.publisher || "").trim().toLowerCase();
  if (pub) {
    list = list.filter((a) => (a.publisher || "").toLowerCase().includes(pub));
  }
  if (filter.size_gt_kb && filter.size_gt_kb > 0) {
    // UI list may still use registry EstimatedSize; copilot filter uses that field only.
    list = list.filter((a) => a.estimated_size_kb >= filter.size_gt_kb!);
  }
  const after = (filter.installed_after || "").trim();
  if (after) {
    list = list.filter((a) => (a.install_date || "") >= after);
  }
  return list;
}

export function CopilotPanel({
  apps,
  aiEnabled,
  onApplyFilter,
  onAnalyze,
  onBatch,
  onForceClean,
}: {
  apps: InstalledApp[];
  aiEnabled: boolean;
  onApplyFilter: (list: InstalledApp[], intent: NlIntent) => void;
  onAnalyze: (app: InstalledApp) => void;
  onBatch: (apps: InstalledApp[], intent: NlIntent) => void;
  onForceClean: (app: InstalledApp) => void;
}) {
  const L = t();
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const [busy, setBusy] = useState(false);
  const [intent, setIntent] = useState<NlIntent | null>(null);
  const [matches, setMatches] = useState<InstalledApp[]>([]);

  const parse = async () => {
    if (!aiEnabled) {
      toast.info(L.copilotNeedAi);
      return;
    }
    const text = q.trim();
    if (!text) return;
    setBusy(true);
    setIntent(null);
    setMatches([]);
    try {
      const parsed = await invoke<NlIntent>("ai_parse_intent", {
        text,
        appNames: apps.map((a) => a.name).slice(0, 40),
      });
      const hit = applyFilter(apps, parsed.filter || {});
      setIntent(parsed);
      setMatches(hit);
      if (hit.length === 0) toast.info(L.copilotNoMatch);
    } catch (e) {
      toast.error(L.aiFailed);
      void formatError(e);
    } finally {
      setBusy(false);
    }
  };

  const actionLabel = (a: string) =>
    a === "analyze"
      ? L.copilotActionAnalyze
      : a === "batch_uninstall"
        ? L.copilotActionBatch
        : a === "force_clean"
          ? L.copilotActionForce
          : L.copilotActionList;

  return (
    <div style={{ marginBottom: 10, flexShrink: 0 }}>
      <button
        style={css.btnSm}
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
      >
        {L.copilotOpen}
      </button>
      {open && (
        <div
          style={{
            ...css.card,
            marginTop: 8,
            padding: 12,
            display: "flex",
            flexDirection: "column",
            gap: 10,
          }}
        >
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
            <input
              style={{ ...css.input, flex: "1 1 240px", height: 34 }}
              placeholder={L.copilotPlaceholder}
              value={q}
              onChange={(e) => setQ(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void parse();
              }}
            />
            <button style={css.btnSm} disabled={busy || !q.trim()} onClick={() => void parse()}>
              {busy ? L.copilotParsing : L.copilotParse}
            </button>
          </div>
          {intent && (
            <div
              style={{
                border: "1px solid var(--border)",
                borderRadius: 10,
                padding: "10px 12px",
                background: "var(--surface-2)",
                fontSize: 12.5,
                lineHeight: 1.5,
              }}
            >
              <div style={{ fontWeight: 700, marginBottom: 4 }}>
                {L.copilotPlan} · {actionLabel(intent.action)}
              </div>
              <div style={{ color: "var(--muted)" }}>{intent.note || "—"}</div>
              <div style={{ color: "var(--muted)", marginTop: 4 }}>
                {L.colName}: {matches.length} / {apps.length}
              </div>
              <div
                style={{
                  display: "flex",
                  gap: 8,
                  flexWrap: "wrap",
                  marginTop: 10,
                }}
              >
                <button
                  style={css.btnSm}
                  disabled={matches.length === 0}
                  onClick={() => onApplyFilter(matches, intent)}
                >
                  {L.copilotApply}
                </button>
                {intent.action === "analyze" && matches[0] && (
                  <button style={css.btnSm} onClick={() => onAnalyze(matches[0])}>
                    {L.copilotRunAnalyze}
                  </button>
                )}
                {intent.action === "batch_uninstall" && matches.length > 0 && (
                  <button
                    style={{ ...css.btnSm, color: "var(--danger)", borderColor: "var(--danger)" }}
                    onClick={() => onBatch(matches, intent)}
                  >
                    {L.copilotRunBatch}
                  </button>
                )}
                {intent.action === "force_clean" && matches[0] && (
                  <button
                    style={{ ...css.btnSm, color: "var(--danger)", borderColor: "var(--danger)" }}
                    onClick={() => onForceClean(matches[0])}
                  >
                    {L.copilotRunForce}
                  </button>
                )}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
