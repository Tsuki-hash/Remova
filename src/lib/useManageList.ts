import { useCallback, useEffect, useState } from "react";
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

  const reload = useCallback(async () => {
    setBusy(true);
    try {
      setItems(await LIST_FN[tab]());
    } catch (e) {
      onError?.(formatError(e));
    } finally {
      setBusy(false);
    }
  }, [tab, onError]);

  useEffect(() => {
    void reload();
  }, [reload]);

  return { items, busy, reload };
}
