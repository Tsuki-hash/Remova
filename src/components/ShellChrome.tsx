import type { UpdateInfo } from "../lib/updateCheck";
import { openUpdateDownload } from "../lib/updateCheck";
import { api } from "../lib/api";
import { t } from "../i18n";
import { useRef, useState } from "react";
import { installUpdate, type UpdateProgress } from "../lib/installUpdate";
import { toast } from "../lib/toast";

const linkBtn = {
  border: "none",
  background: "transparent",
  color: "var(--accent)",
  cursor: "pointer",
  fontSize: 12,
  padding: 0,
  fontWeight: 650,
} as const;

export function ShellStatus({
  disk,
  admin,
  onElevate,
}: {
  disk: string;
  admin: boolean | null;
  onElevate: () => void;
}) {
  const L = t();
  return (
    <>
      {disk && <span title={L.disk}>{disk}</span>}
      {admin === false && (
        <button
          style={{
            border: "none",
            background: "transparent",
            color: "var(--warn-ink)",
            cursor: "pointer",
            fontSize: 12,
            padding: 0,
            fontWeight: 600,
          }}
          title={L.adminChipHint}
          onClick={onElevate}
        >
          {L.nonAdmin}
        </button>
      )}
    </>
  );
}

export function ShellFooter({
  updateInfo,
  selectedCount,
  totalCount,
  estimating,
  estimateLabel,
  monitoring,
  busyRef,
  updateBlocked,
}: {
  updateInfo: UpdateInfo | null;
  selectedCount?: number;
  totalCount?: number;
  estimating?: boolean;
  estimateLabel?: string;
  monitoring?: boolean;
  busyRef?: { current: boolean };
  updateBlocked?: boolean;
}) {
  const L = t();
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const installing = useRef(false);
  const blocked = useRef(false);
  blocked.current = Boolean(updateBlocked);
  const onInstall = async () => {
    if (!updateInfo || !busyRef || installing.current) return;
    installing.current = true;
    try {
      await installUpdate(updateInfo, busyRef, () => blocked.current, setProgress);
    } catch (e) {
      toast.error(L.versionInstallFailed, { detail: String(e), sticky: true });
    } finally {
      installing.current = false;
    }
  };
  return (
    <>
      {progress && (
        <div role="dialog" aria-modal="true" aria-label={L.versionInstall}
          style={{ position: "fixed", inset: 0, zIndex: 10000, background: "rgba(0,0,0,.65)", display: "grid", placeItems: "center" }}>
          <div style={{ background: "var(--surface)", color: "var(--fg)", padding: 24, borderRadius: 12 }} aria-live="polite">
            {progress.phase === "checking" ? L.versionCheck : progress.phase === "installing" ? L.versionInstalling : L.versionDownloading}
            {progress.percent !== undefined && ` ${progress.percent}%`}
          </div>
        </div>
      )}
      {updateInfo?.installInApp && busyRef && (
        <button style={linkBtn} disabled={!!progress || updateBlocked} onClick={() => void onInstall()}>
          {L.versionNew} v{updateInfo.version} → {L.versionInstall}
        </button>
      )}
      {updateInfo && (
        <button
          style={linkBtn}
          title={updateInfo.downloadUrl || updateInfo.url}
          onClick={() => {
            const target = updateInfo.downloadUrl || updateInfo.url;
            void api.openPath(target).catch(() => {
              if (updateInfo.downloadUrl) void openUpdateDownload(updateInfo);
            });
          }}
        >
          {L.versionNew} v{updateInfo.version} → {L.versionDownload}
        </button>
      )}
      {selectedCount !== undefined && <span>{L.footerSelected(selectedCount)}</span>}
      {totalCount !== undefined && <span>{L.footerTotal(totalCount)}</span>}
      {estimating && estimateLabel && <span style={{ color: "var(--accent)" }}>{estimateLabel}</span>}
      {monitoring && <span>{L.monitorRunning}</span>}
    </>
  );
}
