import type { UpdateInfo } from "../lib/updateCheck";
import { openUpdateDownload } from "../lib/updateCheck";
import { api } from "../lib/api";
import { t } from "../i18n";

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
            color: "var(--warn)",
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
}: {
  updateInfo: UpdateInfo | null;
  selectedCount?: number;
  totalCount?: number;
  estimating?: boolean;
  estimateLabel?: string;
  monitoring?: boolean;
}) {
  const L = t();
  return (
    <>
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
