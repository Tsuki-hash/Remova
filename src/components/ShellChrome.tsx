import type { UpdateInfo } from "../lib/updateCheck";
import { openUpdateDownload } from "../lib/updateCheck";
import { t } from "../i18n";
import { useRef, useState } from "react";
import {
  installUpdate,
  progressAnnouncement,
  updateErrorMessage,
  type UpdateProgress,
} from "../lib/installUpdate";
import { useDialogFocus } from "../lib/useDialogFocus";
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

/** Off-screen live region: announced but never painted. */
const visuallyHidden = {
  position: "absolute",
  width: 1,
  height: 1,
  margin: -1,
  padding: 0,
  overflow: "hidden",
  clip: "rect(0 0 0 0)",
  whiteSpace: "nowrap",
  border: 0,
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
  const overlayRef = useRef<HTMLDivElement>(null);
  // Same dialog contract as every other modal: focus moves in, Tab cycles,
  // focus is restored on close. Escape is swallowed on purpose — an update
  // that was already confirmed must not be cancellable from the backdrop.
  useDialogFocus(!!progress, overlayRef);
  const installing = useRef(false);
  const blocked = useRef(false);
  blocked.current = Boolean(updateBlocked);
  const onInstall = async () => {
    if (!updateInfo || !busyRef || installing.current) return;
    installing.current = true;
    try {
      await installUpdate(updateInfo, busyRef, () => blocked.current, setProgress);
    } catch (e) {
      // Raw error goes to the console for diagnosis; the toast stays localized.
      console.error("[update] install failed", e);
      const raw = e instanceof Error ? e.message : typeof e === "string" ? e : String(e);
      if (raw === "update:busy") {
        // Nothing failed — another task holds the slot. Not an error.
        toast.info(L.taskBusy);
      } else {
        toast.error(L.versionInstallFailed, { detail: updateErrorMessage(e), sticky: true });
      }
    } finally {
      installing.current = false;
    }
  };
  const disabledReason = progress
    ? progressAnnouncement(progress, L)
    : updateBlocked
      ? L.monitorRunning
      : undefined;
  return (
    <>
      {progress && (
        <div role="dialog" aria-modal="true" aria-label={L.versionInstall}
          style={{ position: "fixed", inset: 0, zIndex: 10000, background: "rgba(0,0,0,.65)", display: "grid", placeItems: "center" }}>
          <div ref={overlayRef} tabIndex={-1}
            onKeyDown={(e) => {
              if (e.key === "Escape") e.stopPropagation();
            }}
            style={{ background: "var(--surface)", color: "var(--fg)", padding: 24, borderRadius: 12 }}>
            {/* Screen readers get the stepped announcement (hidden); the
                visible line keeps the exact raw percent — no doubled %. */}
            <span aria-live="polite" style={visuallyHidden}>
              {progressAnnouncement(progress, L)}
            </span>
            {progress.phase === "checking"
              ? L.versionCheck
              : progress.phase === "installing"
                ? L.versionInstalling
                : L.versionDownloading}
            {progress.percent !== undefined && ` ${progress.percent}%`}
          </div>
        </div>
      )}
      {updateInfo?.installInApp && busyRef && (
        <button style={linkBtn} disabled={!!progress || updateBlocked} title={disabledReason} onClick={() => void onInstall()}>
          {L.versionInstall} v{updateInfo.version}
        </button>
      )}
      {updateInfo && (
        <button
          style={linkBtn}
          title={updateInfo.downloadUrl || updateInfo.url}
          onClick={() => {
            // openUpdateDownload falls back to the release page when the
            // asset URL cannot be opened.
            void openUpdateDownload(updateInfo).catch(() => {
              toast.error(L.versionCheckFailed);
            });
          }}
        >
          {L.versionDownload} v{updateInfo.version}
        </button>
      )}
      {selectedCount !== undefined && <span>{L.footerSelected(selectedCount)}</span>}
      {totalCount !== undefined && <span>{L.footerTotal(totalCount)}</span>}
      {estimating && estimateLabel && <span style={{ color: "var(--accent)" }}>{estimateLabel}</span>}
      {monitoring && <span>{L.monitorRunning}</span>}
    </>
  );
}
