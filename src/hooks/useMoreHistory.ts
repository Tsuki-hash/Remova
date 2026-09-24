import { useCallback, useState } from "react";
import { api } from "../lib/api";
import { formatError } from "../lib/format";
import { requestConfirm } from "../lib/confirm";
import { toast } from "../lib/toast";
import { t } from "../i18n";
import type { HistoryEntry } from "../types";

export function useMoreHistory(onError: (msg: string) => void) {
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [histQ, setHistQ] = useState("");
  const [openHistory, setOpenHistory] = useState(false);

  const loadHistory = useCallback(async () => {
    try {
      const h = await api.history();
      setHistory(h);
      setOpenHistory(true);
    } catch (e) {
      onError(formatError(e));
    }
  }, [onError]);

  const deleteHistory = useCallback(
    async (id: string) => {
      const L = t();
      const row = history.find((h) => h.id === id);
      const label = row?.app_name ?? id;
      const ok = await requestConfirm({
        title: L.deleteHistory,
        message: L.deleteHistoryConfirm(label),
        confirmLabel: L.deleteHistory,
        danger: true,
      });
      if (!ok) return;
      try {
        await api.deleteHistory([id]);
        const h = await api.history();
        setHistory(h);
        toast.success(L.historyDeleted);
      } catch (e) {
        onError(formatError(e));
      }
    },
    [history, onError],
  );

  const clearHistory = useCallback(async () => {
    const L = t();
    const ok = await requestConfirm({
      title: L.clearHistory,
      message: L.clearHistoryConfirm,
      confirmLabel: L.clearHistory,
      danger: true,
    });
    if (!ok) return;
    try {
      await api.clearHistory();
      setHistory([]);
      toast.success(L.historyCleared);
    } catch (e) {
      onError(formatError(e));
    }
  }, [onError]);

  const exportCsv = useCallback(async () => {
    const L = t();
    try {
      const csv = await api.exportHistoryCsv();
      const blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "remova-history.csv";
      document.body.appendChild(a);
      a.click();
      a.remove();
      URL.revokeObjectURL(url);
      toast.success(L.exportCsv);
    } catch (e) {
      onError(formatError(e));
    }
  }, [onError]);

  const closeHistory = useCallback(() => setOpenHistory(false), []);

  return {
    history,
    histQ,
    setHistQ,
    openHistory,
    loadHistory,
    exportCsv,
    deleteHistory,
    clearHistory,
    closeHistory,
  };
}
