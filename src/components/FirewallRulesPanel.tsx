import { useCallback, useEffect, useState } from "react";
import { CloseGlyph } from "./ui/Glyph";
import { api } from "../lib/api";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { backendErrorText } from "../lib/backendText";
import { toast } from "../lib/toast";
import { requestConfirmEx } from "../lib/confirm";
import type { FirewallRule } from "../types";

/**
 * M3 (SOP §8-1): firewall rules whose program file no longer exists. The scan
 * is read-only; deletion re-verifies every value name server-side and exports
 * the registry values to a Safety Vault session first.
 */
export function FirewallRulesPanel({
  onClose,
  onError,
}: {
  onClose: () => void;
  onError: (msg: string) => void;
}) {
  const L = t();
  const [rows, setRows] = useState<FirewallRule[] | null>(null);
  const [omitted, setOmitted] = useState(0);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);

  const scan = useCallback(async () => {
    setBusy(true);
    try {
      const result = await api.firewallOrphans();
      setRows(result.rules);
      setOmitted(result.omitted);
      setSelected(new Set());
    } catch (e) {
      onError(formatError(e));
    } finally {
      setBusy(false);
    }
  }, [onError]);

  useEffect(() => {
    void scan();
  }, [scan]);

  const toggle = (name: string, on: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (on) next.add(name);
      else next.delete(name);
      return next;
    });
  };

  const removeSelected = async () => {
    if (busy || selected.size === 0) return;
    const { ok } = await requestConfirmEx({
      title: L.firewallDeleteSelected,
      message: L.firewallDeleteConfirm(selected.size),
      confirmLabel: L.firewallDeleteSelected,
      danger: true,
    });
    if (!ok) return;
    setBusy(true);
    try {
      const report = await api.deleteFirewallRules([...selected]);
      if (report.failed.length > 0) {
        toast.error(L.firewallDeletePartial(report.failed.length), {
          detail: report.backup_dir,
        });
      } else {
        toast.success(L.firewallDeleteOk, { detail: report.backup_dir });
      }
      await scan();
    } catch (e) {
      const msg = backendErrorText(formatError(e));
      if (msg.toLowerCase().includes("administrator privileges")) {
        toast.error(L.firewallNeedAdmin);
      } else {
        toast.error(L.firewallDeleteFailed(msg));
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <section style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <strong>{L.firewallTitle}</strong>
        <span style={css.muted}>{L.firewallHint}</span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          <button style={{ ...css.btnGhost, height: 30 }} disabled={busy} onClick={() => void scan()}>
            {L.manageReload}
          </button>
          <button
            style={{ ...css.btnGhost, height: 30, color: "var(--danger-text)" }}
            disabled={busy || selected.size === 0}
            onClick={() => void removeSelected()}
          >
            {L.firewallDeleteSelected} ({selected.size})
          </button>
          <button style={{ ...css.btnGhost, height: 30 }} onClick={onClose} aria-label={L.panelClose}>
            <CloseGlyph />
          </button>
        </div>
      </div>
      {rows === null ? (
        <div style={{ ...css.muted, padding: "10px 0" }}>{L.loadingGeneric}</div>
      ) : rows.length === 0 ? (
        <div style={{ ...css.muted, padding: "10px 0" }}>{L.firewallEmpty}</div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 10 }}>
          {rows.map((row) => (
            <label
              key={row.value_name}
              style={{
                display: "flex",
                gap: 10,
                alignItems: "flex-start",
                padding: "8px 10px",
                borderRadius: 8,
                border: "1px solid var(--border)",
                background: "var(--surface-2)",
                cursor: "pointer",
              }}
            >
              <input
                type="checkbox"
                checked={selected.has(row.value_name)}
                disabled={busy}
                onChange={(e) => toggle(row.value_name, e.target.checked)}
              />
              <div style={{ minWidth: 0, flex: 1 }}>
                <div style={{ fontWeight: 600 }}>{row.rule_name}</div>
                <div style={{ ...css.muted, fontSize: 11.5 }}>
                  {L.firewallApp}: <span style={{ fontFamily: "var(--mono)" }}>{row.app_path}</span>
                </div>
                <div style={{ ...css.muted, fontSize: 11 }}>
                  {row.direction === "Out" ? L.firewallDirectionOut : L.firewallDirectionIn}
                  {" · "}
                  {row.action === "Block" ? L.firewallActionBlock : L.firewallActionAllow}
                  {row.protocol ? ` · ${row.protocol}` : ""}
                </div>
              </div>
            </label>
          ))}
        </div>
      )}
      {omitted > 0 && (
        <div style={{ ...css.muted, marginTop: 6, fontSize: 12 }}>{L.firewallOmitted(omitted)}</div>
      )}
    </section>
  );
}
