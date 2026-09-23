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
  const [openRestore, setOpenRestore] = useState(false);

  const loadRestore = useCallback(async () => {
    setRestoreMsgs([]);
    setRestorePick("");
    setSessions([]);
    try {
      const list = await api.backupSessions();
      setSessions(list);
      if (list[0]) setRestorePick(list[0].name);
      setOpenRestore(true);
    } catch (e) {
      onError(formatError(e));
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
      message: L.restoreConfirm,
      confirmLabel: L.restoreRun,
      danger: true,
    });
    if (!ok) return;
    setRestoreBusy(true);
    setRestoreMsgs([]);
    try {
      const msgs = await api.restoreSessionByName(restorePick);
      setRestoreMsgs(msgs.length ? msgs : ["ok"]);
      toast.success(L.restoreResult);
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
