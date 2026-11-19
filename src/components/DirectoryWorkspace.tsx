import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { formatSize, t } from "../i18n";
import { api } from "../lib/api";
import { appKey } from "../lib/appKey";
import { formatError } from "../lib/format";
import { dialogLayer, useDialogFocus } from "../lib/useDialogFocus";
import { scanCancelled, useAssociationScan } from "../hooks/useAssociationScan";
import { cssStyles as css } from "../styles";
import type { DirSizeRow, FolderReferences, FolderReferenceSource, InstalledApp } from "../types";
import { AppDetailPanel } from "./AppDetailPanel";
import { DiskRadarPanel } from "./DiskRadarPanel";
import { ScanProgressBar } from "./ScanProgressBar";
import { BackButton } from "./ui/BackButton";
import "./DirectoryWorkspace.css";

export type FolderInspection = { path: string; measurement?: DirSizeRow; returnToRadar?: boolean };
type ReturnPosition = { opener: HTMLButtonElement; scroll: { element: HTMLElement; top: number }[] };

function capturePosition(opener: HTMLButtonElement, dialog: HTMLDivElement | null): ReturnPosition {
  const scroll: ReturnPosition["scroll"] = [];
  for (let element = opener.parentElement; element; element = element.parentElement) {
    if (element === dialog || /^(auto|scroll)$/.test(getComputedStyle(element).overflowY)) {
      scroll.push({ element, top: element.scrollTop });
    }
    if (element === dialog) break;
  }
  return { opener, scroll };
}

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
  const radarPosition = useRef<ReturnPosition | null>(null);
  const referencePosition = useRef<ReturnPosition | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [opened, setOpened] = useState<({ inspection: FolderInspection; key: string } & ReturnPosition) | null>(null);
  const app = opened && opened.inspection === inspection
    ? apps.find(item => appKey(item) === opened.key)
    : undefined;
  const back = () => {
    if (app && opened) {
      referencePosition.current = opened;
      setOpened(null);
    }
    else if (inspection && radar) onBack();
    else onClose();
  };
  useDialogFocus(true, dialog, dialogLayer.overview, back);
  useLayoutEffect(() => {
    const position = referencePosition.current ?? (!inspection && radar ? radarPosition.current : null);
    referencePosition.current = null;
    if (!inspection) radarPosition.current = null;
    if (position?.opener.isConnected) {
      for (const { element, top } of position.scroll) element.scrollTo({ top, behavior: "instant" });
      position.opener.focus({ preventScroll: true });
    } else {
      if (dialog.current) dialog.current.scrollTop = 0;
      backButton.current?.focus();
    }
  }, [inspection, app, radar]);

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
            label={app ? L.folderInspectBack : inspection && (radar || inspection.returnToRadar) ? L.folderInspectRadar : L.returnSoftwareList} />
        </header>
        {error && <p role="alert" className="folder-reference-incomplete">{error}</p>}
        {radar && (
          <div className="directory-radar-view" hidden={Boolean(inspection)}>
            <DiskRadarPanel
              onError={setError}
              onInspect={(row, opener) => {
                radarPosition.current = capturePosition(opener, dialog.current);
                onInspect({ path: row?.path ?? "", measurement: row });
              }}
            />
          </div>
        )}
        {inspection && (
          <div hidden={Boolean(app)}>
            <FolderReferencePanel key={inspection.path} inspection={inspection} apps={apps}
              onInspect={path => onInspect({ path, returnToRadar: inspection.returnToRadar })}
              onOpenPath={openPath}
              onApp={(item, opener) => setOpened({ inspection, key: appKey(item), ...capturePosition(opener, dialog.current) })}
            />
          </div>
        )}
        {app && (
          <AppDetailPanel
            app={app}
            sizeText={app.estimated_size_kb > 0 ? formatSize(app.estimated_size_kb) : "—"}
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
  onApp: (app: InstalledApp, opener: HTMLButtonElement) => void;
}) {
  const L = t();
  const { path, measurement } = inspection;
  const [input, setInput] = useState(path);
  const [result, setResult] = useState<FolderReferences | null>(null);
  const [checkedOnCancel, setCheckedOnCancel] = useState<FolderReferenceSource[]>([]);
  const request = useRef(0);
  const { runReadOnly, cancel, progress } = useAssociationScan();
  const [failed, setFailed] = useState(false);
  const [choosingFolder, setChoosingFolder] = useState(false);
  const [pickerFailed, setPickerFailed] = useState(false);
  const mounted = useRef(false);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  const check = useCallback(() => {
    if (!path) return;
    const requestId = ++request.current;
    setResult(null);
    setFailed(false);
    setCheckedOnCancel([]);
    let checked: FolderReferenceSource[] = [];
    void runReadOnly(async id => {
      const outcome = await api.inspectFolderReferences(path, id);
      if ("checked" in outcome) {
        checked = outcome.checked;
        throw new Error("scan:cancelled");
      }
      return outcome;
    }, value => value.references.length)
      .then(setResult)
      .catch(error => {
        if (requestId !== request.current) return;
        if (scanCancelled(error)) setCheckedOnCancel(checked);
        else setFailed(true);
      });
  }, [path, runReadOnly]);
  // Restarting for a different folder unmounts this panel and cancels its job.
  useEffect(() => { check(); }, [check]);
  const sources = {
    installation: L.folderInspectInstallation,
    shortcut: L.folderInspectShortcut,
    service: L.folderInspectService,
  };
  const running = progress?.status === "running" || progress?.status === "stopping";
  const chooseFolder = async () => {
    if (choosingFolder) return;
    setChoosingFolder(true);
    setPickerFailed(false);
    try {
      const picked = await api.pickReferenceFolder();
      if (!mounted.current || !picked) return;
      if (picked !== path) onInspect(picked);
      else {
        setInput(picked);
        if (!running) check();
      }
    } catch {
      if (mounted.current) setPickerFailed(true);
    } finally {
      if (mounted.current) setChoosingFolder(false);
    }
  };
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
        <button type="button" style={css.btnGhost} disabled={choosingFolder} onClick={() => void chooseFolder()}>
          {L.folderInspectChoose}
        </button>
        <button type="submit" style={css.btn} disabled={running && input.trim() === path}>{L.folderInspectRun}</button>
      </form>
      <p className="folder-reference-note">{L.folderInspectScope}</p>
      {pickerFailed && <p role="alert">{L.folderInspectPickerFailed}</p>}
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
      {/* Keep the live region present before the result arrives. */}
      <div role="status" aria-live="polite" aria-atomic="true">
        {progress?.status === "cancelled" && <p>
          {L.folderInspectCancelled}{" "}
          {checkedOnCancel.length ? L.folderInspectChecked(checkedOnCancel.map(source => sources[source]).join(", "))
            : L.folderInspectNoneChecked}
        </p>}
        {result && result.incomplete.length > 0 && (
          <p className="folder-reference-incomplete">
            {L.folderInspectPartial(result.incomplete.map(source => sources[source]).join(", "))}
          </p>
        )}
        {result?.references.length === 0 && <p>{L.folderInspectEmpty}</p>}
      </div>
      {failed && <p role="alert">{L.folderInspectFailed}</p>}
      {result && (
        <>
          {result.references.length > 0 && (
            <ul className="folder-reference-list">
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
                    {app && <button type="button" style={css.btnGhost} onClick={event => onApp(app, event.currentTarget)}>
                      {L.folderInspectShowApp}
                    </button>}
                  </li>
                );
              })}
            </ul>
          )}
          {result.omitted > 0 && <p className="folder-reference-note">{L.scanOmitted(result.omitted)}</p>}
        </>
      )}
    </div>
  );
}
