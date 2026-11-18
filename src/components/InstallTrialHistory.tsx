import { useEffect, useState } from "react";
import { currentLang, t } from "../i18n";
import { api, type InstallTrialRecord, type MonitorDiff } from "../lib/api";
import { pathKey } from "../lib/cleanupPlan";
import { cssStyles as css } from "../styles";
import type { InstalledApp } from "../types";

function observedRows(record: InstallTrialRecord, app?: InstalledApp) {
  const directory = pathKey(app?.install_location ?? "");
  const registry = app?.registry_key.toLowerCase();
  return [
    ...record.diff.added_files.map(path => {
      const key = pathKey(path);
      const matches = /^[a-z]:\\[^\\]+/.test(directory) && (key === directory || key.startsWith(`${directory}\\`));
      return { path, evidence: matches ? "directory" as const : undefined };
    }),
    ...record.diff.added_reg_values.map(path => ({
      path, evidence: registry && path.toLowerCase() === registry ? "registry" as const : undefined,
    })),
  ];
}

export function InstallTrialHistory({ app, refresh }: { app?: InstalledApp; refresh?: MonitorDiff | null }) {
  const L = t();
  const [open, setOpen] = useState(false);
  const [records, setRecords] = useState<InstallTrialRecord[] | "error" | null>(null);
  useEffect(() => {
    if (!open) return;
    let current = true;
    setRecords(null);
    void api.installTrialRecords().then(
      rows => { if (current) setRecords(rows); },
      () => { if (current) setRecords("error"); },
    );
    return () => { current = false; };
  }, [open, refresh]);
  const visible = Array.isArray(records) ? records.map(record => ({ record, rows: observedRows(record, app) }))
    .filter(({ rows }) => !app || rows.some(row => row.evidence)) : [];
  const clock = new Intl.DateTimeFormat(currentLang(), { dateStyle: "short", timeStyle: "short" });
  return (
    <details style={{ marginTop: 12, overflowWrap: "anywhere" }}
      onToggle={event => setOpen(event.currentTarget.open)}>
      <summary style={{ color: "var(--accent)", fontWeight: 600, cursor: "pointer" }}>{L.trialHistory}</summary>
      {open && (
        <>
          {records === null && <p role="status" style={css.muted}>{L.loadingPage}</p>}
          {records === "error" && <p role="alert">{L.trialLoadFailed}</p>}
          {Array.isArray(records) && visible.length === 0 && (
            <p style={css.muted}>{app ? L.trialNoMatches : L.trialNoRecords}</p>
          )}
          {visible.map(({ record, rows }) => {
            const start = new Date(record.trial.started_at * 1000);
            const end = new Date(record.finished_at * 1000);
            const { diff } = record;
            return (
              <details key={record.trial.id} style={{ margin: "8px 0", padding: 8, border: "1px solid var(--border)", borderRadius: 8 }}>
                <summary style={{ cursor: "pointer" }}>
                  <strong>{record.trial.name}</strong>{" "}
                  <time style={css.muted} dateTime={start.toISOString()}>{clock.format(start)}</time>
                </summary>
                <p style={css.muted}>{L.trialReadOnly} {L.trialEnded(clock.format(end))}</p>
                {diff.recovered_read_only && <p style={css.muted}>{L.monitorRecoveryReadOnly}</p>}
                {diff.walk_degraded && <p style={css.muted}>{L.monitorDegraded}</p>}
                {Boolean(diff.files_truncated || diff.reg_truncated) && (
                  <p style={css.muted}>{L.monitorTruncated(diff.files_truncated ?? 0, diff.reg_truncated ?? 0)}</p>
                )}
                <details>
                  <summary>{L.trialScope}</summary>
                  <ul style={css.mono}>
                    {[...record.trial.file_roots, ...record.trial.registry_roots].map(root => <li key={root}>{root}</li>)}
                  </ul>
                </details>
                {rows.length ? (
                  <ul style={{ paddingInlineStart: 22 }}>
                    {rows.map(({ path, evidence }) => (
                      <li key={path} style={{ marginTop: 6 }}>
                        <span style={css.muted}>{evidence === "registry" ? L.trialRegistry
                          : evidence === "directory" ? L.trialDirectory : L.trialUnconfirmed}</span>
                        <div style={css.mono}>{path}</div>
                      </li>
                    ))}
                  </ul>
                ) : <p style={css.muted}>{L.monitorEmpty}</p>}
              </details>
            );
          })}
        </>
      )}
    </details>
  );
}
