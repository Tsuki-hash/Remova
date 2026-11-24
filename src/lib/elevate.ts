import { api } from "./api";
import { t } from "../i18n";
import { requestConfirmEx } from "./confirm";
import { toast } from "./toast";
import { formatError } from "./format";

/** True when a backend refusal says the action needs administrator rights. */
export function needsAdminRights(error: unknown): boolean {
  const raw = error instanceof Error ? error.message : String(error);
  return raw.toLowerCase().includes("administrator privileges");
}

/** Unknown is not a refusal; leave the decision to the backend in that case. */
export async function isKnownNonElevated(): Promise<boolean> {
  try {
    return (await api.isElevated()) === false;
  } catch {
    return false;
  }
}

/**
 * Offer an elevated restart before an admin-only action or after an admin-rights refusal.
 * The caller supplies the feature-specific reason. Returns true when the restart was initiated.
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
