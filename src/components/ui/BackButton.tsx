import type { ComponentProps } from "react";
import { t } from "../../i18n";
import { cssStyles as css } from "../../styles";

export function BackButton({ label = t().navBack, style, ...button }: Omit<ComponentProps<"button">, "children"> & { label?: string }) {
  return (
    <button type="button" aria-label={label} {...button} style={{ ...css.btnGhost, whiteSpace: "nowrap", ...style }}>
      <span aria-hidden="true">←</span> {label}
    </button>
  );
}
