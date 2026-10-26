import { t } from "../i18n";
import { cssStyles as css } from "../styles";

const KEY = "remova_onboarded_v1";

/** Whether the first-run safety card was already dismissed. */
export function loadOnboarded(): boolean {
  try {
    return localStorage.getItem(KEY) === "seen";
  } catch {
    // Storage disabled — treat as seen so the card never pesters a broken setup.
    return true;
  }
}

export function markOnboarded() {
  try {
    localStorage.setItem(KEY, "seen");
  } catch {
    // Session-only dismissal is fine; the card just reappears next launch.
  }
}

/**
 * One-time, non-blocking safety-model card above the software list. Three
 * principles only; the full story lives in the features themselves. Reopenable
 * from More → help without clearing the persisted flag.
 */
export function OnboardingCard({ onClose }: { onClose: () => void }) {
  const L = t();
  return (
    <section
      aria-label={L.onboardingTitle}
      style={{ ...css.card, padding: 16, marginBottom: 10 }}
    >
      <strong style={{ fontSize: 13 }}>{L.onboardingTitle}</strong>
      <p style={{ ...css.muted, margin: "6px 0 6px" }}>{L.onboardingIntro}</p>
      <ul style={{ margin: 0, paddingLeft: 18, display: "grid", gap: 4 }}>
        <li>{L.onboardingPoint1}</li>
        <li>{L.onboardingPoint2}</li>
        <li>{L.onboardingPoint3}</li>
      </ul>
      <div style={{ display: "flex", justifyContent: "flex-end", marginTop: 10 }}>
        <button
          type="button"
          style={{ ...css.btn, height: 32, fontSize: 12.5 }}
          onClick={onClose}
        >
          {L.onboardingDismiss}
        </button>
      </div>
    </section>
  );
}
