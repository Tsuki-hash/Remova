import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import { formatError } from "../lib/format";
import { requestConfirm } from "../lib/confirm";
import { toast } from "../lib/toast";
import { t, type Strings } from "../i18n";
import type { BackupSession, RestorePreview } from "../lib/api";

const restoreResultPrefixes = [
  ["restored PATH entry ", "restoreDone", true],
  ["restored ", "restoreDone", true],
  ["imported ", "restoreDone", true],
  ["PATH entry already present: ", "restoreAlreadyPresent", true],
  ["PATH restore failed for ", "restorePathFailed", false],
  ["registry import failed ", "restoreImportFailed", false],
  ["restore failed for ", "restoreFileFailed", false],
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

function describeRestoreResult(message: string, L: Strings) {
  const match = restoreResultPrefixes.find(([prefix]) => message.startsWith(prefix));
  if (match) {
    const [prefix, label, complete] = match;
    return { text: `${L[label]}: ${message.slice(prefix.length)}`, complete };
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
  const [restoreMsgs, setRestoreMsgs] = useState<string[]>([]);
  const [restoreLoading, setRestoreLoading] = useState(false);
  const [openRestore, setOpenRestore] = useState(false);
  const [restoreLoadError, setRestoreLoadError] = useState<string | null>(null);
  const [restorePreview, setRestorePreview] = useState<RestorePreview | null>(null);
  const [unsealedSessions, setUnsealedSessions] = useState(0);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewTick, setPreviewTick] = useState(0);
  const operation = useRef(false);
  const previewSequence = useRef(0);

  useEffect(() => {
    const sequence = ++previewSequence.current;
    let current = true;
    setRestorePreview(null);
    setPreviewError(null);
    setPreviewLoading(!!restorePick && openRestore);
    if (restorePick && openRestore) {
      void api.previewRestore(restorePick).then(preview => {
        if (current && sequence === previewSequence.current) setRestorePreview(preview);
      }).catch(error => {
        if (current && sequence === previewSequence.current) setPreviewError(formatError(error));
      }).finally(() => { if (current && sequence === previewSequence.current) setPreviewLoading(false); });
    }
    return () => { current = false; };
  }, [restorePick, openRestore, previewTick]);

  const loadRestore = useCallback(async (requestedName?: string) => {
    if (operation.current) return;
    operation.current = true;
    ++previewSequence.current;
    setRestoreMsgs([]);
    setRestorePick("");
    setSessions([]);
    setUnsealedSessions(0);
    setRestoreLoading(true);
    setRestoreLoadError(null);
    try {
      const list = await api.backupSessions();
      // an aborted backup leaves a session directory with no seal. It can
      // never be restored (`validate_session` rejects it with `seal:missing`),
      // so it must not become the default selection and then fail on confirm.
      const restorable = list.filter(session => session.sealed);
      setUnsealedSessions(list.length - restorable.length);
      setSessions(restorable);
      setOpenRestore(true);
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
      operation.current = true;
      setRestoreBusy(true);
      const L = t();
      try {
        const ok = await requestConfirm({
          title: L.deleteSession,
          message: L.deleteSessionConfirm(name),
          confirmLabel: L.deleteSession,
          danger: true,
        });
        if (!ok) return;
        await api.deleteBackupSession(name);
        const list = await api.backupSessions();
        const restorable = list.filter(session => session.sealed);
        setUnsealedSessions(list.length - restorable.length);
        setSessions(restorable);
        setRestorePick((cur) => (cur === name ? (restorable[0]?.name ?? "") : cur));
      } catch (e) {
        onError(formatError(e));
      } finally {
        operation.current = false;
        setRestoreBusy(false);
      }
    },
    [onError],
  );

  const runRestore = useCallback(async () => {
    const L = t();
    if (!restorePick || operation.current) return;
    operation.current = true;
    setRestoreBusy(true);
    setRestoreMsgs([]);
    try {
      // Invalidate any older preview still in flight for this same selection.
      ++previewSequence.current;
      setPreviewLoading(true);
      let preview: RestorePreview;
      try {
        preview = await api.previewRestore(restorePick);
      } catch (error) {
        // A failed pre-confirm preview owns its own surface (alert + retry);
        // the restore-result block must stay reserved for a confirmed run.
        setRestorePreview(null);
        setPreviewError(formatError(error));
        toast.error(formatError(error));
        return;
      } finally {
        setPreviewLoading(false);
      }
      setRestorePreview(preview);
      setPreviewError(null);
      const ok = await requestConfirm({
        title: L.restore,
        message: [L.restoreConfirm(restorePick),
          preview.existing ? L.restoreConflict(preview.existing) : "",
          preview.unavailable ? L.restoreUnavailable(preview.unavailable) : ""].filter(Boolean).join("\n"),
        confirmLabel: L.restoreRun,
        danger: true,
      });
      if (!ok) return;
      const msgs = await api.restoreSessionByName(restorePick);
      const rows = msgs.map((msg) => describeRestoreResult(msg, L));
      setRestoreMsgs(rows.length ? rows.map(({ text }) => text) : [L.restoreNoDetail]);
      if (rows.length && rows.every(({ complete }) => complete)) toast.success(L.restoreDone);
      else toast.info(rows.length ? L.restoreIncomplete : L.restoreNoDetail);
    } catch (e) {
      setRestoreMsgs([formatError(e)]);
      toast.error(formatError(e));
    } finally {
      operation.current = false;
      setRestoreBusy(false);
    }
  }, [restorePick]);

  const selectRestore = useCallback((name: string) => {
    if (!operation.current) setRestorePick(name);
  }, []);
  const retryPreview = useCallback(() => {
    if (!operation.current) setPreviewTick((v) => v + 1);
  }, []);
  const closeRestore = useCallback(() => {
    if (!operation.current) setOpenRestore(false);
  }, []);

  return {
    sessions,
    unsealedSessions,
    restoreLoading,
    restoreLoadError,
    restorePreview: restorePreview?.name === restorePick ? restorePreview : null,
    previewError,
    previewLoading,
    restorePick,
    setRestorePick: selectRestore,
    restoreBusy,
    restoreMsgs,
    openRestore,
    loadRestore,
    deleteSession,
    runRestore,
    retryPreview,
    closeRestore,
  };
}
