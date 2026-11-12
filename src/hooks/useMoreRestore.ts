import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../lib/api";
import { formatError } from "../lib/format";
import { requestConfirm } from "../lib/confirm";
import { toast } from "../lib/toast";
import { t, type Strings } from "../i18n";
import type { RestorePreview } from "../lib/api";
import { backupDeleteExpectation, canPreviewBackup, type BackupSession } from "../lib/backupSession";
import { isNamedStreamRefusal } from "../lib/backendText";

const restoreResultPrefixes = [
  ["restored PATH entry ", "restoreDone", true],
  ["restored PATH scope: ", "restoreDone", true],
  ["restored ", "restoreDone", true],
  ["imported ", "restoreDone", true],
  ["PATH entry already present: ", "restoreAlreadyPresent", true],
  ["PATH scope already matches backup: ", "restoreAlreadyPresent", true],
  ["skipped changed PATH scope: ", "restorePathScopeChanged", false],
  ["PATH restore failed for ", "restorePathFailed", false],
  ["PATH scope restore failed for ", "restorePathScopeFailed", false],
  ["registry import failed ", "restoreImportFailed", false],
  ["restore directory may be incomplete: ", "restoreDirectoryIncomplete", false],
  ["restore directory interrupted: ", "restoreDirectoryInterrupted", false],
  ["restore directory kept conflicts: ", "restoreDirectoryConflicts", false],
  ["restore failed for ", "restoreFileFailed", false],
  ["skipped existing restore target: ", "restoreTargetOccupied", false],
  ["skipped backup entry with no recorded digest: ", "restoreSkippedNoDigest", false],
  ["skipped modified backup entry (digest mismatch): ", "restoreSkippedDigestMismatch", false],
  ["skipped backup directory too large to verify: ", "restoreSkippedVerificationLimit", false],
  ["skipped unrecorded backup file: ", "restoreSkippedUnrecordedFile", false],
  ["skipped unrecorded backup directory: ", "restoreSkippedUnrecordedDirectory", false],
  ["skipped backup directory with missing recorded file: ", "restoreSkippedIncompleteDirectory", false],
  ["skipped missing backup entry: ", "restoreSkippedMissingEntry", false],
  ["skipped protected restore target: ", "restoreSkippedProtectedTarget", false],
  ["skipped unsealed restore target: ", "restoreSkippedUnsealedTarget", false],
  ["skipped reparse backup entry: ", "restoreSkippedReparseEntry", false],
  ["skipped restore target without parent: ", "restoreSkippedUnsafeParent", false],
  ["skipped restore target without parent pins: ", "restoreSkippedUnsafeParent", false],
  ["skipped redirected restore target: ", "restoreSkippedRedirectedTarget", false],
  ["skipped reparse restore target: ", "restoreSkippedRedirectedTarget", false],
] as const;

const restoreStoppedBeforeChanges = new Set([
  "restore cancelled before any changes",
  "restore:cancelled",
  "cleanup cancelled",
]);

function describeRestoreResult(message: string, L: Strings) {
  if (restoreStoppedBeforeChanges.has(message)) return { text: L.restoreCancelledBeforeChanges, complete: false };
  const match = restoreResultPrefixes.find(([prefix]) => message.startsWith(prefix));
  if (match) {
    const [prefix, label, complete] = match;
    const summary = !complete && isNamedStreamRefusal(message) ? L.backendBackupNamedStreams : L[label];
    return { text: `${summary}: ${message.slice(prefix.length)}`, complete };
  }
  switch (message) {
    case "restore cancelled":
      return { text: L.restoreFilesCancelled, complete: false };
    case "restore cancelled before remaining PATH entries":
      return { text: L.restorePathCancelled, complete: false };
    case "restore cancelled before remaining registry imports":
      return { text: L.restoreCancelled, complete: false };
    default:
      // Preserve diagnostics from other backend versions without counting them
      // as confirmed writes.
      return { text: message || L.restoreNoDetail, complete: false };
  }
}

export function useMoreRestore(onError: (msg: string) => void) {
  const [sessions, setSessions] = useState<BackupSession[]>([]);
  const [restorePick, setRestorePick] = useState("");
  const [restoreBusy, setRestoreBusy] = useState(false);
  const [restoreResult, setRestoreResult] = useState<(
    { name: string; messages: string[] } | { name: string; error: unknown }
  ) | null>(null);
  const [runningRestore, setRunningRestore] = useState<string | null>(null);
  const [cancelRequested, setCancelRequested] = useState(false);
  const [restoreLoading, setRestoreLoading] = useState(false);
  const [openRestore, setOpenRestore] = useState(false);
  const [previewVisible, setPreviewVisibility] = useState(false);
  const [restoreLoadError, setRestoreLoadError] = useState<string | null>(null);
  const [restorePreview, setRestorePreview] = useState<RestorePreview | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewTick, setPreviewTick] = useState(0);
  const operation = useRef(false);
  const activeRestore = useRef<{ id: string; cancelling: boolean } | null>(null);
  const previewSequence = useRef(0);
  const pendingConfirmation = useRef<{ id: string; cancelled: boolean; settled: boolean } | null>(null);

  const cancelPendingConfirmation = useCallback(() => {
    const pending = pendingConfirmation.current;
    if (!pending || pending.cancelled) return;
    pending.cancelled = true;
    if (!pending.settled) void api.cancelRestorePreview(pending.id).catch(() => {});
  }, []);

  const setPreviewVisible = useCallback((visible: boolean) => {
    if (!visible) cancelPendingConfirmation();
    setPreviewVisibility(visible);
  }, [cancelPendingConfirmation]);

  useEffect(() => cancelPendingConfirmation, [cancelPendingConfirmation]);

  useEffect(() => {
    const sequence = ++previewSequence.current;
    let current = true;
    setRestorePreview(null);
    setPreviewError(null);
    setPreviewLoading(!!restorePick && openRestore && previewVisible);
    if (restorePick && openRestore && previewVisible) {
      const id = crypto.randomUUID();
      let settled = false;
      void api.previewRestore(restorePick, id).then(preview => {
        if (current && sequence === previewSequence.current) setRestorePreview(preview);
      }).catch(error => {
        if (current && sequence === previewSequence.current) setPreviewError(formatError(error));
      }).finally(() => {
        settled = true;
        if (current && sequence === previewSequence.current) setPreviewLoading(false);
      });
      return () => {
        current = false;
        if (!settled) void api.cancelRestorePreview(id).catch(() => {});
      };
    }
    return () => { current = false; };
  }, [restorePick, openRestore, previewVisible, previewTick]);

  const loadRestore = useCallback(async (requestedName?: string) => {
    if (operation.current) return;
    operation.current = true;
    ++previewSequence.current;
    setRestorePick("");
    setSessions([]);
    setOpenRestore(true);
    setRestoreLoading(true);
    setRestoreLoadError(null);
    try {
      const list = await api.backupSessions();
      const restorable = list.filter(canPreviewBackup);
      setSessions(list);
      if (requestedName && !restorable.some(session => session.name === requestedName)) {
        setRestoreLoadError(t().restoreSessionMissing);
        onError(t().restoreSessionMissing);
        return;
      }
      setRestorePick(requestedName ?? restorable[0]?.name ?? "");
    } catch (e) {
      setRestoreLoadError(formatError(e));
      onError(formatError(e));
    } finally {
      operation.current = false;
      setRestoreLoading(false);
    }
  }, [onError]);

  const deleteSession = useCallback(
    async (name: string) => {
      if (operation.current) return;
      const session = sessions.find(candidate => candidate.name === name);
      if (!session) return;
      const status = backupDeleteExpectation(session, restorePreview?.name === name);
      if (!status) return;
      operation.current = true;
      setRestoreBusy(true);
      const L = t();
      let deleted = false;
      try {
        const ok = await requestConfirm({
          title: L.deleteSession,
          message: L.deleteSessionConfirm(name),
          confirmLabel: L.deleteSession,
          danger: true,
        });
        if (!ok) return;
        await api.deleteBackupSession(name, status);
        deleted = true;
        const remaining = sessions.filter(session => session.name !== name);
        setSessions(remaining);
        setRestorePick(cur => cur === name ? remaining.find(canPreviewBackup)?.name ?? "" : cur);
        const list = await api.backupSessions();
        const restorable = list.filter(canPreviewBackup);
        setSessions(list);
        setRestoreLoadError(null);
        setRestorePick(cur => {
          if (cur === name) return restorable[0]?.name ?? "";
          return restorable.some(session => session.name === cur) ? cur : "";
        });
      } catch (e) {
        const error = formatError(e);
        if (deleted) setRestoreLoadError(error);
        onError(error);
      } finally {
        operation.current = false;
        setRestoreBusy(false);
      }
    },
    [onError, restorePreview, sessions],
  );

  const runRestore = useCallback(async () => {
    const L = t();
    if (!restorePick || operation.current) return;
    operation.current = true;
    setRestoreBusy(true);
    const pending = { id: crypto.randomUUID(), cancelled: false, settled: false };
    pendingConfirmation.current = pending;
    try {
      // Invalidate any older preview still in flight for this same selection.
      ++previewSequence.current;
      setPreviewLoading(true);
      let preview: RestorePreview;
      try {
        preview = await api.previewRestore(restorePick, pending.id);
      } catch (error) {
        if (pending.cancelled) return;
        // A failed pre-confirm preview owns its own surface (alert + retry);
        // the restore-result block must stay reserved for a confirmed run.
        setRestorePreview(null);
        setPreviewError(formatError(error));
        toast.error(formatError(error));
        return;
      } finally {
        pending.settled = true;
        if (!pending.cancelled) setPreviewLoading(false);
      }
      if (pending.cancelled) return;
      setRestorePreview(preview);
      setPreviewError(null);
      const ok = await requestConfirm({
        title: L.restore,
        message: [L.restoreConfirm(restorePick),
          preview.existing ? L.restoreConflict(preview.existing) : "",
          preview.resumable ? L.restoreResumable(preview.resumable) : "",
          preview.unavailable ? L.restoreUnavailable(preview.unavailable) : ""].filter(Boolean).join("\n"),
        confirmLabel: L.restoreRun,
        danger: true,
      });
      if (!ok || pending.cancelled) return;
      pendingConfirmation.current = null;
      const id = crypto.randomUUID();
      activeRestore.current = { id, cancelling: false };
      setRestoreResult(null);
      setRunningRestore(restorePick);
      const messages = await api.restoreSessionByName(restorePick, id);
      const locale = t();
      setRestoreResult({ name: restorePick, messages });
      if (messages.length && messages.every(msg => describeRestoreResult(msg, locale).complete)) {
        toast.success(locale.restoreDone);
      } else {
        toast.info(messages.length ? locale.restoreIncomplete : locale.restoreNoDetail);
      }
    } catch (e) {
      const raw = typeof e === "string" ? e : e instanceof Error ? e.message : "";
      if (restoreStoppedBeforeChanges.has(raw)) {
        setRestoreResult({ name: restorePick, messages: [raw] });
        toast.info(t().restoreCancelledBeforeChanges);
      } else {
        setRestoreResult({ name: restorePick, error: e });
        toast.error(formatError(e));
      }
    } finally {
      if (pendingConfirmation.current === pending) pendingConfirmation.current = null;
      if (activeRestore.current) setPreviewTick(v => v + 1);
      activeRestore.current = null;
      setRunningRestore(null);
      setCancelRequested(false);
      operation.current = false;
      setRestoreBusy(false);
    }
  }, [restorePick]);

  const cancelRestore = useCallback(async () => {
    const current = activeRestore.current;
    if (!current || current.cancelling) return;
    current.cancelling = true;
    setCancelRequested(true);
    try {
      if (await api.cancelRestore(current.id)) return;
      if (activeRestore.current === current) toast.error(t().restoreCancelUnavailable);
    } catch (error) {
      if (activeRestore.current === current) toast.error(formatError(error));
    }
    if (activeRestore.current === current) {
      current.cancelling = false;
      setCancelRequested(false);
    }
  }, []);

  const selectRestore = useCallback((name: string) => {
    if (operation.current || !sessions.some(session => session.name === name && canPreviewBackup(session))) return;
    setRestoreLoadError(null);
    setRestorePick(name);
  }, [sessions]);
  const retryPreview = useCallback(() => {
    if (!operation.current) setPreviewTick((v) => v + 1);
  }, []);
  const closeRestore = useCallback(() => {
    if (!operation.current) setOpenRestore(false);
  }, []);

  const L = t();
  const restoreMsgs = useMemo(() => {
    if (!restoreResult) return [];
    if ("error" in restoreResult) return [formatError(restoreResult.error)];
    if (!restoreResult.messages.length) return [L.restoreNoDetail];
    return restoreResult.messages.map(msg => describeRestoreResult(msg, L).text);
  }, [L, restoreResult]);

  return {
    sessions,
    restoreLoading,
    restoreLoadError,
    restorePreview: restorePreview?.name === restorePick ? restorePreview : null,
    previewError,
    previewLoading,
    restorePick,
    setRestorePick: selectRestore,
    restoreBusy,
    restoreMsgs,
    restoreResultName: restoreResult?.name,
    runningRestore,
    cancelRequested,
    cancelRestore,
    openRestore,
    loadRestore,
    deleteSession,
    runRestore,
    retryPreview,
    setPreviewVisible,
    closeRestore,
  };
}
