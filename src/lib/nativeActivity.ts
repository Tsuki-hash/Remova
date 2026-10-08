// Share an exclusive update slot with every native operation (including restore
// and service management, which do not all use the software page's busyRef).

/** Stable token thrown when the update slot is held — callers and the error
 * formatter both match on this exact string (unified IPC error contract). */
export const UPDATE_BUSY = "update:busy";

/** Read-only commands that stay available while an update is in flight —
 * the shell keeps rendering (app list, disk meter, history) even though
 * every mutating operation is blocked by the update slot. */
const READ_ONLY_COMMANDS = new Set([
  "list_installed_apps",
  "cancel_association_scan",
  "association_scan_progress",
  "is_elevated",
  "disk_usage",
  "get_ai_config",
  "load_ignore",
  "list_cleanup_history",
  "list_backup_sessions",
  "preview_restore_session",
  "check_github_latest",
  "online_update_supported",
  "list_local_drives",
  "list_top_dir_sizes",
  "list_dir_children",
  "rank_idle_apps",
  "app_icon_data",
  "estimate_dir_size_kb",
  "list_startup_items",
  "list_services",
  "list_scheduled_tasks",
  "verify_cleanup_leftovers",
]);

let active = 0;
let writes = 0;
let updating = false;

export function hasPendingNativeWrites(): boolean {
  return updating || writes > 0;
}

export async function trackNativeCall<T>(
  call: () => Promise<T>,
  command?: string,
): Promise<T> {
  if (updating && !(command && READ_ONLY_COMMANDS.has(command))) {
    throw new Error(UPDATE_BUSY);
  }
  active++;
  const writing = !command || !READ_ONLY_COMMANDS.has(command);
  if (writing) writes++;
  try {
    return await call();
  } finally {
    active--;
    if (writing) writes--;
  }
}

export function acquireUpdateSlot(): (() => void) | null {
  if (updating || active > 0) return null;
  updating = true;
  return () => { updating = false; };
}
