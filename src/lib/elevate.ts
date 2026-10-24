import { api } from "./api";
import { t } from "../i18n";
import { requestConfirmEx } from "./confirm";
import { toast } from "./toast";
import { formatError } from "./format";

/** True when a backend refusal says the action needs administrator rights. */
export function needsAdminRights(raw: string): boolean {
  return raw.toLowerCase().includes("administrator privileges");
}

/**
 * Offer an elevated restart after an admin-rights refusal. The caller supplies
 * the feature-specific reason as the message; the shared title and button
 * carry the restart semantics. Returns true when the restart was initiated.
 */
export async function offerElevatedRestart(message: string): Promise<boolean> {
  const L = t();
  const { ok } = await requestConfirmEx({
    title: L.elevateAskTitle,
    message,
    confirmLabel: L.elevateAskOk,
  });
  if (!ok) return false;
  try {
    await api.elevateRestart();
    return true;
  } catch (e) {
    toast.error(formatError(e, "elevate"));
    return false;
  }
}
