import { useState } from "react";
import { CloseGlyph, Deco } from "./ui/Glyph";
import { api } from "../lib/api";
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
    list = list.filter((a) => a.estimated_size_kb >= filter.size_gt_kb!);
  }
  const after = (filter.installed_after || "").trim();
  if (after) {
    list = list.filter((a) => (a.install_date || "") >= after);
  }
  return list;
}

/** Offline keyword fallback when no model is configured (P0 Copilot always usable). */
export function ruleParseFilter(apps: InstalledApp[], text: string): {
  list: InstalledApp[];
  intent: NlIntent;
} {
  const raw = text.trim();
  const lower = raw.toLowerCase();
  const words = lower
    .split(/[\s,，、]+/)
    .map((w) => w.trim())
    .filter((w) => w.length >= 2 && !/^(的|软件|找出|查找|卸载|清理|残留|相关|and|the|apps?)$/i.test(w));

  let list = apps;
  const nameLike = words.find((w) => apps.some((a) => a.name.toLowerCase().includes(w)));
  if (nameLike) {
    list = list.filter((a) => a.name.toLowerCase().includes(nameLike));
  }
  const pubLike = words.find((w) =>
    list.some((a) => (a.publisher || "").toLowerCase().includes(w)),
  );
  if (pubLike) {
    list = list.filter((a) => (a.publisher || "").toLowerCase().includes(pubLike));
  }

  let sizeGtKb: number | null = null;
  const gb = lower.match(/(\d+(?:\.\d+)?)\s*g\s*b?/);
  const mb = lower.match(/(\d+(?:\.\d+)?)\s*m\s*b?/);
  if (gb) sizeGtKb = Math.round(Number(gb[1]) * 1024 * 1024);
  else if (mb) sizeGtKb = Math.round(Number(mb[1]) * 1024);
  if (sizeGtKb && sizeGtKb > 0) {
    list = list.filter((a) => a.estimated_size_kb >= sizeGtKb!);
  }

  const action: NlIntent["action"] = /分析|analyze/i.test(raw)
    ? "analyze"
    : /批量|batch/i.test(raw)
      ? "batch_uninstall"
      : /强制|force/i.test(raw)
        ? "force_clean"
        : "list";

  return {
    list,
    intent: {
      action,
      filter: {
        name_like: nameLike || null,
        publisher: pubLike || null,
        size_gt_kb: sizeGtKb,
        installed_after: null,
      },
      include_leftovers: /残留|leftover/i.test(raw),
      note: raw,
    },
  };
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
  const [q, setQ] = useState("");
  const [busy, setBusy] = useState(false);
  const [intent, setIntent] = useState<NlIntent | null>(null);
  const [matches, setMatches] = useState<InstalledApp[]>([]);

  const parse = async () => {
    const text = q.trim();
    if (!text) return;
    setBusy(true);
    setIntent(null);
    setMatches([]);
    try {
      if (!aiEnabled) {
        const { list, intent: ruleIntent } = ruleParseFilter(apps, text);
        setIntent(ruleIntent);
        setMatches(list);
        if (list.length === 0) toast.info(L.copilotNoMatch);
        else toast.info(L.conclusionSourceRule);
        return;
      }
      const parsed = await api.aiParseIntent(text, apps.map((a) => a.name).slice(0, 40));
      const hit = applyFilter(apps, parsed.filter || {});
      setIntent(parsed);
      setMatches(hit);
      if (hit.length === 0) toast.info(L.copilotNoMatch);
    } catch (e) {
      toast.error(formatError(e));
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
      <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
        <span
          style={{
            fontSize: 11,
            fontWeight: 650,
            color: "var(--muted)",
            letterSpacing: 0.2,
            flexShrink: 0,
          }}
        >
          <Deco ch="✦" /> {L.smartFilter}
        </span>
        <input
          style={{ ...css.input, flex: "1 1 220px", height: 32, minWidth: 180 }}
          placeholder={L.copilotInlinePlaceholder}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void parse();
          }}
          aria-label={L.smartFilter}
        />
        <button
          style={{ ...css.btnSm, height: 32 }}
          disabled={busy || !q.trim()}
          onClick={() => void parse()}
        >
          {busy ? L.copilotParsing : L.copilotParse}
        </button>
        {intent && (
          <button
            type="button"
            style={{ ...css.btnSm, height: 32 }}
            onClick={() => {
              setIntent(null);
              setMatches([]);
            }}
          >
            <CloseGlyph />
          </button>
        )}
      </div>
      <div style={{ display: "flex", gap: 6, flexWrap: "wrap", marginTop: 8, alignItems: "center" }}>
        <span style={{ ...css.muted, fontSize: 11.5 }}>{L.smartFilterExamplesTitle}</span>
        {[
          L.smartFilterExample1,
          L.smartFilterExample2,
          L.smartFilterExample3,
          L.smartFilterExample4,
        ].map((ex) => (
          <button
            key={ex}
            type="button"
            style={{
              ...css.chip,
              fontFamily: "inherit",
              cursor: "pointer",
              background: "var(--surface)",
            }}
            onClick={() => setQ(ex)}
          >
            {ex}
          </button>
        ))}
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
            marginTop: 8,
          }}
        >
          <div style={{ fontWeight: 700, marginBottom: 4, display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
            <span>
              {L.copilotPlan} · {actionLabel(intent.action)}
            </span>
            <span
              style={{
                fontSize: 10.5,
                fontWeight: 650,
                color: aiEnabled ? "var(--accent)" : "var(--muted)",
                background: aiEnabled ? "var(--accent-soft)" : "var(--surface)",
                border: "1px solid var(--border)",
                borderRadius: 999,
                padding: "1px 7px",
              }}
            >
              {aiEnabled ? L.conclusionSourceAi : L.conclusionSourceRule}
            </span>
          </div>
          <div style={{ color: "var(--muted)" }}>{intent.note || "—"}</div>
          <div style={{ color: "var(--muted)", marginTop: 4 }}>
            {L.colName}: {matches.length} / {apps.length}
            {matches.length > 0 && (
              <span style={{ marginLeft: 8 }}>
                {matches
                  .slice(0, 4)
                  .map((m) => m.name)
                  .join(" · ")}
                {matches.length > 4 ? " …" : ""}
              </span>
            )}
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
              <button style={css.btnSm} onClick={() => onAnalyze(matches[0]!)}>
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
                onClick={() => onForceClean(matches[0]!)}
              >
                {L.copilotRunForce}
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
