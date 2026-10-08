import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import { formatError } from "./format";
import type { ManageItem } from "../types";

export type ManageTabId = "startup" | "services" | "tasks";

const LIST_FN: Record<ManageTabId, () => Promise<ManageItem[]>> = {
  startup: api.listStartupItems,
  services: api.listServices,
  tasks: api.listScheduledTasks,
};

export function useManageList(tab: ManageTabId, onError?: (msg: string) => void) {
  const [items, setItems] = useState<ManageItem[]>([]);
  const [busy, setBusy] = useState(false);
  const requestSeq = useRef(0);
  const busyRef = useRef(false);

  const reload = useCallback(async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    const seq = ++requestSeq.current;
    setBusy(true);
    setItems([]);
    try {
      const list = await LIST_FN[tab]();
      if (seq === requestSeq.current) setItems(list);
    } catch (e) {
      if (seq === requestSeq.current) onError?.(formatError(e));
    } finally {
      if (seq === requestSeq.current) {
        busyRef.current = false;
        setBusy(false);
      }
    }
  }, [tab, onError]);

  useEffect(() => {
    void reload();
    return () => { requestSeq.current++; busyRef.current = false; };
  }, [reload]);

  return { items, busy, reload };
}
