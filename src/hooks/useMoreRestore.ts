import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import { formatError } from "../lib/format";
import { requestConfirm } from "../lib/confirm";
import { toast } from "../lib/toast";
import { t } from "../i18n";
import type { BackupSession, RestorePreview } from "../lib/api";

export function useMoreRestore(onError: (msg: string) => void) {
  const [sessions, setSessions] = useState<BackupSession[]>([]);
  const [restorePick, setRestorePick] = useState("");
  const [restoreBusy, setRestoreBusy] = useState(false);
  const [restoreMsgs, setRestoreMsgs] = useState<string[]>([]);
  const [restoreLoading, setRestoreLoading] = useState(false);
  const [openRestore, setOpenRestore] = useState(false);
  const [restoreLoadError, setRestoreLoadError] = useState<string | null>(null);
  const [restorePreview, setRestorePreview] = useState<RestorePreview | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
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
  }, [restorePick, openRestore]);

  const loadRestore = useCallback(async () => {
    if (operation.current) return;
    setRestoreMsgs([]);
    setRestorePick("");
    setSessions([]);
    setRestoreLoading(true);
    setRestoreLoadError(null);
    try {
      const list = await api.backupSessions();
      setSessions(list);
      if (list[0]) setRestorePick(list[0].name);
      setOpenRestore(true);
    } catch (e) {
      setRestoreLoadError(formatError(e));
      onError(formatError(e));
    } finally {
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
        setSessions(list);
        setRestorePick((cur) => (cur === name ? (list[0]?.name ?? "") : cur));
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
      const preview = await api.previewRestore(restorePick).catch(error => {
        setRestorePreview(null);
        setPreviewError(formatError(error));
        throw error;
      }).finally(() => setPreviewLoading(false));
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
      setRestoreMsgs(msgs.length ? msgs.map((msg) => {
        if (msg.startsWith("restored ")) return `${L.restoreDone}: ${msg.slice(msg.startsWith("restored PATH entry ") ? 20 : 9)}`;
        if (msg.startsWith("imported ")) return `${L.restoreDone}: ${msg.slice(9)}`;
        if (msg.startsWith("PATH entry already present: ")) return `${L.restoreAlreadyPresent}: ${msg.slice(28)}`;
        if (msg.startsWith("skipped ")) return `${L.restoreSkipped}: ${msg.slice(msg.indexOf(": ") + 2)}`;
        if (msg.startsWith("restore failed for ")) return `${L.restoreIncomplete}: ${msg.slice(19).split(": ")[0]}`;
        return L.restoreNoDetail;
      }) : [L.restoreNoDetail]);
      const complete = msgs.length > 0 && msgs.every((msg) => msg.startsWith("restored ") || msg.startsWith("imported ") || msg.startsWith("PATH entry already present: "));
      if (complete) toast.success(L.restoreDone);
      else toast.info(msgs.length ? L.restoreIncomplete : L.restoreNoDetail);
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
  const closeRestore = useCallback(() => {
    if (!operation.current) setOpenRestore(false);
  }, []);

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
    openRestore,
    loadRestore,
    deleteSession,
    runRestore,
    closeRestore,
  };
}
