import { useCallback, useState } from "react";
import { api } from "../lib/api";
import { formatError } from "../lib/format";
import { requestConfirm } from "../lib/confirm";
import { toast } from "../lib/toast";
import { t } from "../i18n";
import type { BackupSession } from "../lib/api";

export function useMoreRestore(onError: (msg: string) => void) {
  const [sessions, setSessions] = useState<BackupSession[]>([]);
  const [restorePick, setRestorePick] = useState("");
  const [restoreBusy, setRestoreBusy] = useState(false);
  const [restoreMsgs, setRestoreMsgs] = useState<string[]>([]);
  const [restoreLoading, setRestoreLoading] = useState(false);
  const [openRestore, setOpenRestore] = useState(false);
  const [restoreLoadError, setRestoreLoadError] = useState<string | null>(null);

  const loadRestore = useCallback(async () => {
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
      const L = t();
      const ok = await requestConfirm({
        title: L.deleteSession,
        message: L.deleteSessionConfirm(name),
        confirmLabel: L.deleteSession,
        danger: true,
      });
      if (!ok) return;
      try {
        await api.deleteBackupSession(name);
        const list = await api.backupSessions();
        setSessions(list);
        setRestorePick((cur) => (cur === name ? (list[0]?.name ?? "") : cur));
      } catch (e) {
        onError(formatError(e));
      }
    },
    [onError],
  );

  const runRestore = useCallback(async () => {
    const L = t();
    if (!restorePick || restoreBusy) return;
    const ok = await requestConfirm({
      title: L.restore,
      message: L.restoreConfirm(restorePick),
      confirmLabel: L.restoreRun,
      danger: true,
    });
    if (!ok) return;
    setRestoreBusy(true);
    setRestoreMsgs([]);
    try {
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
      setRestoreBusy(false);
    }
  }, [restorePick, restoreBusy]);

  const closeRestore = useCallback(() => setOpenRestore(false), []);

  return {
    sessions,
    restoreLoading,
    restoreLoadError,
    restorePick,
    setRestorePick,
    restoreBusy,
    restoreMsgs,
    openRestore,
    loadRestore,
    deleteSession,
    runRestore,
    closeRestore,
  };
}
