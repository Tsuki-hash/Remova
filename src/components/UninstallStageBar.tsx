import { t } from "../i18n";
import { cssStyles as css } from "../styles";

export type UninstallStage = "idle" | "official";

export function UninstallStageBar({ stage }: { stage: UninstallStage }) {
  if (stage === "idle") return null;
  return (
    <p role="status" style={{ ...css.card, padding: "8px 14px", margin: "0 0 8px", flexShrink: 0 }}>
      {t().stageOfficial}
    </p>
  );
}
