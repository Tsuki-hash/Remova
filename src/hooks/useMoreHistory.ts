import { useCallback, useState } from "react";
import { api } from "../lib/api";
import { formatError } from "../lib/format";
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

  const exportCsv = useCallback(async () => {
    const L = t();
    try {
      const csv = await api.exportHistoryCsv();
      const blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "remova-history.csv";
      a.click();
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
    closeHistory,
  };
}
