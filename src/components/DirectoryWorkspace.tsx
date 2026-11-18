import { useCallback, useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { formatSize, t } from "../i18n";
import { api } from "../lib/api";
import { appKey } from "../lib/appKey";
import { formatError } from "../lib/format";
import { dialogLayer, useDialogFocus } from "../lib/useDialogFocus";
import { scanCancelled, useAssociationScan } from "../hooks/useAssociationScan";
import { cssStyles as css } from "../styles";
import type { DirSizeRow, FolderReferences, InstalledApp } from "../types";
import { AppDetailPanel } from "./AppDetailPanel";
import { DiskRadarPanel } from "./DiskRadarPanel";
import { ScanProgressBar } from "./ScanProgressBar";
import { BackButton } from "./ui/BackButton";
import "./DirectoryWorkspace.css";

export type FolderInspection = { path: string; measurement?: DirSizeRow; returnToRadar?: boolean };

export function DirectoryWorkspace({
  radar, inspection, apps, onInspect, onBack, onClose, onOpenSoftware,
}: {
  radar: boolean;
  inspection: FolderInspection | null;
  apps: InstalledApp[];
  onInspect: (inspection: FolderInspection) => void;
  onBack: () => void;
  onClose: () => void;
  onOpenSoftware: (app: InstalledApp, action: "analyze" | "uninstall") => void;
}) {
  const L = t();
  const title = useId();
  const dialog = useRef<HTMLDivElement>(null);
  const backButton = useRef<HTMLButtonElement>(null);
  const [error, setError] = useState<string | null>(null);
  const [opened, setOpened] = useState<{ inspection: FolderInspection; key: string } | null>(null);
  const app = opened && opened.inspection === inspection
    ? apps.find(item => appKey(item) === opened.key)
    : undefined;
  const back = () => {
    if (app) setOpened(null);
    else if (inspection && radar) onBack();
    else onClose();
  };
  useDialogFocus(true, dialog, dialogLayer.overview, back);
  useEffect(() => {
    backButton.current?.focus();
  }, [inspection, app]);

  const openPath = (path: string) => {
    setError(null);
    void api.openPath(path).catch(error => setError(formatError(error)));
  };
  return createPortal(
    <div className="directory-backdrop" style={{ zIndex: dialogLayer.overview }}>
      <div className="directory-dialog" ref={dialog} role="dialog" aria-modal="true" aria-labelledby={title} tabIndex={-1}>
        <header className="directory-heading">
          <h2 id={title}>{app ? L.folderInspectShowApp : inspection ? L.folderInspectTitle : L.diskRadarShortcut}</h2>
          <BackButton ref={backButton} onClick={back}
            label={app ? L.folderInspectBack : inspection && (radar || inspection.returnToRadar) ? L.folderInspectRadar : L.navBack} />
        </header>
        {error && <p role="alert" className="folder-reference-incomplete">{error}</p>}
        {radar && (
          <div hidden={Boolean(inspection)}>
            <DiskRadarPanel
              onError={setError}
              onInspect={row => onInspect({ path: row?.path ?? "", measurement: row })}
            />
          </div>
        )}
        {inspection && (
          <div hidden={Boolean(app)}>
            <FolderReferencePanel key={inspection.path} inspection={inspection} apps={apps}
              onInspect={path => onInspect({ path, returnToRadar: inspection.returnToRadar })}
              onOpenPath={openPath}
              onApp={item => setOpened({ inspection, key: appKey(item) })}
            />
          </div>
        )}
        {app && (
          <AppDetailPanel
            app={app}
            sizeText={app.estimated_size_kb > 0 ? formatSize(app.estimated_size_kb) : "—"}
            onClose={() => setOpened(null)}
            uninstalling={false}
            onAnalyze={item => onOpenSoftware(item, "analyze")}
            onDeepUninstall={item => onOpenSoftware(item, "uninstall")}
            onOpenPath={openPath}
          />
        )}
      </div>
    </div>,
    document.body,
  );
}

function FolderReferencePanel({
  inspection, apps, onInspect, onOpenPath, onApp,
}: {
  inspection: FolderInspection;
  apps: InstalledApp[];
  onInspect: (path: string) => void;
  onOpenPath: (path: string) => void;
  onApp: (app: InstalledApp) => void;
}) {
  const L = t();
  const { path, measurement } = inspection;
  const [input, setInput] = useState(path);
  const [result, setResult] = useState<FolderReferences | null>(null);
  const { runReadOnly, cancel, progress } = useAssociationScan();
  const [failed, setFailed] = useState(false);
  const check = useCallback(() => {
    if (!path) return;
    setResult(null);
    setFailed(false);
    void runReadOnly(id => api.inspectFolderReferences(path, id), value => value.references.length)
      .then(setResult)
      .catch(error => { if (!scanCancelled(error)) setFailed(true); });
  }, [path, runReadOnly]);
  // Restarting for a different folder unmounts this panel and cancels its job.
  useEffect(() => { check(); }, [check]);
  const sources = {
    installation: L.folderInspectInstallation,
    shortcut: L.folderInspectShortcut,
    service: L.folderInspectService,
  };
  const running = progress?.status === "running" || progress?.status === "stopping";
  return (
    <div className="folder-references">
      <form className="folder-reference-form" onSubmit={event => {
        event.preventDefault();
        const next = input.trim();
        if (!next) return;
        if (next !== path) onInspect(next);
        else if (!running) check();
      }}>
        <label>
          {L.folderInspectPath}
          <input style={css.input} value={input} onChange={event => setInput(event.target.value)} spellCheck={false} required />
        </label>
        <button type="submit" style={css.btn} disabled={running && input.trim() === path}>{L.folderInspectRun}</button>
      </form>
      <p className="folder-reference-note">{L.folderInspectScope}</p>
      {path && (
        <div className="folder-reference-path">
          <code>{path}</code>
          {measurement && <span>{measurement.capped
            ? L.detailSizeFloor(formatSize(measurement.size_kb))
            : formatSize(measurement.size_kb)}</span>}
          {result && <button type="button" style={css.btnGhost} onClick={() => onOpenPath(path)}>{L.openLocation}</button>}
        </div>
      )}
      {running && <ScanProgressBar progress={progress} onCancel={cancel} />}
      {progress?.status === "cancelled" && <p role="status">{L.folderInspectCancelled}</p>}
      {failed && <p role="alert">{L.folderInspectFailed}</p>}
      {result && (
        <>
          {result.incomplete.length > 0 && (
            <p className="folder-reference-incomplete" role="status">
              {L.folderInspectPartial(result.incomplete.map(source => sources[source]).join(", "))}
            </p>
          )}
          {result.references.length === 0
            ? <p role="status">{L.folderInspectEmpty}</p>
            : <ul className="folder-reference-list">
                {result.references.map((reference, index) => {
                  const app = reference.app_source && apps.find(item =>
                    item.source === reference.app_source && item.registry_key === reference.location &&
                    item.install_location === reference.target);
                  return (
                    <li key={index}>
                      <div>
                        <strong>{reference.name}</strong>
                        <span className="folder-reference-note">{sources[reference.source]}</span>
                        <code>{reference.target}</code>
                        <details>
                          <summary>{L.detailShow}</summary>
                          <code>{reference.location}</code>
                        </details>
                      </div>
                      {app && <button type="button" style={css.btnGhost} onClick={() => onApp(app)}>{L.folderInspectShowApp}</button>}
                    </li>
                  );
                })}
              </ul>}
          {result.omitted > 0 && <p className="folder-reference-note">{L.scanOmitted(result.omitted)}</p>}
        </>
      )}
    </div>
  );
}
