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
  "is_elevated",
  "disk_usage",
  "get_ai_config",
  "load_ignore",
  "list_cleanup_history",
  "list_backup_sessions",
  "check_github_latest",
  "online_update_supported",
  "list_local_drives",
  "list_top_dir_sizes",
  "list_dir_children",
  "rank_idle_apps",
  "app_icon_data",
]);

let active = 0;
let updating = false;

export async function trackNativeCall<T>(
  call: () => Promise<T>,
  command?: string,
): Promise<T> {
  if (updating && !(command && READ_ONLY_COMMANDS.has(command))) {
    throw new Error(UPDATE_BUSY);
  }
  active++;
  try {
    return await call();
  } finally {
    active--;
  }
}

export function acquireUpdateSlot(): (() => void) | null {
  if (updating || active > 0) return null;
  updating = true;
  return () => { updating = false; };
}
