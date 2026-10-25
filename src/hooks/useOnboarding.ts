import { useCallback, useState } from "react";
import { loadOnboarded, markOnboarded } from "../components/OnboardingCard";

/**
 * First-run safety-card visibility: dismissal persists across launches,
 * reopening from More → help is session-only and never resurrects the card
 * on the next boot.
 */
export function useOnboarding() {
  const [open, setOpen] = useState(() => !loadOnboarded());
  const dismiss = useCallback(() => {
    markOnboarded();
    setOpen(false);
  }, []);
  const reopen = useCallback(() => setOpen(true), []);
  return { open, dismiss, reopen };
}
