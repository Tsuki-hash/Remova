import { useCallback, useEffect, useState } from "react";
import { CloseGlyph } from "./ui/Glyph";
import { api } from "../lib/api";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { backendErrorText } from "../lib/backendText";
import { toast } from "../lib/toast";
import { requestConfirmEx } from "../lib/confirm";
import type { BrokenInstall } from "../types";

/**
 * M1 (SOP §9.7): an MSI product whose install directory was deleted by hand
 * keeps its Uninstall record, so Programs & Features can no longer remove it.
 * Detection is read-only; the first action is re-running the vendor uninstaller.
 */
export function BrokenInstallsPanel({
  onClose,
  onError,
}: {
  onClose: () => void;
  onError: (msg: string) => void;
}) {
  const L = t();
  const [rows, setRows] = useState<BrokenInstall[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [running, setRunning] = useState<string | null>(null);

  const scan = useCallback(async () => {
    setBusy(true);
    try {
      setRows(await api.brokenInstalls());
    } catch (e) {
      onError(formatError(e));
    } finally {
      setBusy(false);
    }
  }, [onError]);

  useEffect(() => {
    void scan();
  }, [scan]);

  const rerun = async (row: BrokenInstall) => {
    if (busy) return;
    setRunning(row.guid);
    setBusy(true);
    try {
      const result = await api.officialUninstall(row.app);
      if (result.ok) {
        toast.success(L.brokenInstallsRerunOk);
        await scan();
      } else {
        toast.error(L.brokenInstallsFailed(backendErrorText(result.message)));
        // The record may still be gone even when the uninstaller exits non-zero
        // (1605/1603); a rescan tells the truth.
        const next = await api.brokenInstalls();
        setRows(next);
        if (next.some((r) => r.guid === row.guid)) {
          toast.info(L.brokenInstallsStillBroken);
        }
      }
    } catch (e) {
      onError(formatError(e));
    } finally {
      setRunning(null);
      setBusy(false);
    }
  };

  const remove = async (row: BrokenInstall) => {
    if (busy) return;
    const { ok } = await requestConfirmEx({
      title: L.brokenInstallsRemove,
      message: L.brokenInstallsRemoveConfirm(row.app.name),
      confirmLabel: L.brokenInstallsRemove,
      danger: true,
    });
    if (!ok) return;
    setRunning(row.guid);
    setBusy(true);
    try {
      const report = await api.cleanupBrokenInstall(
        row.app.name,
        row.app.registry_key,
        row.guid,
      );
      if (report.failed.length > 0) {
        toast.error(L.brokenInstallsRemovePartial(report.failed.length), {
          detail: report.backup_dir,
        });
      } else {
        toast.success(L.brokenInstallsRemoveOk, { detail: report.backup_dir });
      }
      await scan();
    } catch (e) {
      toast.error(L.brokenInstallsRemoveFailed(formatError(e)));
    } finally {
      setRunning(null);
      setBusy(false);
    }
  };

  return (
    <section style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <strong>{L.brokenInstallsTitle}</strong>
        <span style={css.muted}>{L.brokenInstallsHint}</span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          <button style={{ ...css.btnGhost, height: 30 }} disabled={busy} onClick={() => void scan()}>
            {L.manageReload}
          </button>
          <button style={{ ...css.btnGhost, height: 30 }} onClick={onClose} aria-label={L.panelClose}>
            <CloseGlyph />
          </button>
        </div>
      </div>
      {rows === null ? (
        <div style={{ ...css.muted, padding: "10px 0" }}>{L.loadingGeneric}</div>
      ) : rows.length === 0 ? (
        <div style={{ ...css.muted, padding: "10px 0" }}>{L.brokenInstallsEmpty}</div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 10 }}>
          {rows.map((row) => (
            <div
              key={row.guid}
              style={{
                display: "flex",
                gap: 10,
                alignItems: "center",
                padding: "8px 10px",
                borderRadius: 8,
                border: "1px solid var(--border)",
                background: "var(--surface-2)",
              }}
            >
              <div style={{ minWidth: 0, flex: 1 }}>
                <div style={{ fontWeight: 600 }}>{row.app.name}</div>
                <div style={{ ...css.muted, fontSize: 11.5 }}>
                  {L.brokenInstallsMissingDir}:{" "}
                  <span style={{ fontFamily: "var(--mono)" }}>{row.app.install_location}</span>
                </div>
                <div style={{ ...css.muted, fontSize: 11 }}>
                  {L.brokenInstallsGuid}: <span style={{ fontFamily: "var(--mono)" }}>{row.guid}</span>
                </div>
              </div>
              <button
                style={{ ...css.btnGhost, height: 28 }}
                disabled={busy}
                onClick={() => void rerun(row)}
              >
                {running === row.guid ? L.brokenInstallsRunning : L.brokenInstallsRerun}
              </button>
              <button
                style={{ ...css.btnGhost, height: 28, color: "var(--danger-text)" }}
                disabled={busy}
                onClick={() => void remove(row)}
              >
                {L.brokenInstallsRemove}
              </button>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
