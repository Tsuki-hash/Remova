import type { ReactNode } from "react";
import { cssStyles as css } from "../styles";

export function Section({
  title,
  hint,
  extra,
  children,
  columns = false,
}: {
  title: string;
  hint?: string;
  extra?: ReactNode;
  children: ReactNode;
  columns?: boolean;
}) {
  return (
    <section className="remova-tool-section">
      <header>
        <h2>{title}</h2>
        {hint && <span style={{ ...css.muted, fontSize: 12 }}>{hint}</span>}
        {extra && <span style={{ marginLeft: "auto" }}>{extra}</span>}
      </header>
      <div className={columns ? "remova-tool-columns" : undefined}>
        {children}
      </div>
    </section>
  );
}
