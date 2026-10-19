import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import { formatError } from "./format";
import type { ManageItem } from "../types";
import { useAssociationScan, scanCancelled } from "../hooks/useAssociationScan";
import { useRequestSeq } from "../hooks/useRequestSeq";

export type ManageTabId = "startup" | "services" | "tasks";

const LIST_FN: Record<ManageTabId, (scanId?: number) => Promise<ManageItem[]>> = {
  startup: api.listStartupItems,
  services: api.listServices,
  tasks: api.listScheduledTasks,
};

export function useManageList(tab: ManageTabId, onError?: (msg: string) => void) {
  const [items, setItems] = useState<ManageItem[]>([]);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const { next: nextRequest, isCurrent: isCurrentRequest, invalidate: invalidateRequest } = useRequestSeq();
  const { runItems, cancel, progress } = useAssociationScan();

  const reload = useCallback(async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    const seq = nextRequest();
    setBusy(true);
    setItems([]);
    try {
      const list = await runItems(LIST_FN[tab]);
      if (isCurrentRequest(seq)) setItems(list);
    } catch (e) {
      if (isCurrentRequest(seq) && !scanCancelled(e)) onError?.(formatError(e));
    } finally {
      if (isCurrentRequest(seq)) {
        busyRef.current = false;
        setBusy(false);
      }
    }
  }, [tab, onError, runItems, nextRequest, isCurrentRequest]);

  useEffect(() => {
    void reload();
    // Unmounting retires every in-flight request so a late list cannot publish.
    return () => { invalidateRequest(); busyRef.current = false; void cancel().catch(() => {}); };
  }, [reload, cancel, invalidateRequest]);

  return { items, busy, reload, cancel, progress };
}
