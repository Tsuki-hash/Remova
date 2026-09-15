import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatError } from "./format";
import type { ManageItem } from "../components/ManagePanel";

export type ManageTabId = "startup" | "services" | "tasks";

const LIST_CMD: Record<ManageTabId, string> = {
  startup: "list_startup_items",
  services: "list_services",
  tasks: "list_scheduled_tasks",
};

export function useManageList(tab: ManageTabId, onError?: (msg: string) => void) {
  const [items, setItems] = useState<ManageItem[]>([]);
  const [busy, setBusy] = useState(false);

  const reload = useCallback(async () => {
    setBusy(true);
    try {
      const list = await invoke<ManageItem[]>(LIST_CMD[tab]);
      setItems(list);
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
