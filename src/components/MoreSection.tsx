import type { ReactNode } from "react";
import { cssStyles as css } from "../styles";

export function Section({
  title,
  hint,
  extra,
  children,
}: {
  title: string;
  hint?: string;
  extra?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="remova-tool-section" style={{ display: "flex", flexDirection: "column", gap: 12, marginBottom: 24 }}>
      <header
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: 10,
          paddingBottom: 4,
        }}
      >
        <span style={{ fontWeight: 600, fontSize: 13, letterSpacing: 0.1 }}>{title}</span>
        {hint && <span style={{ ...css.muted, fontSize: 12 }}>{hint}</span>}
        {extra && <span style={{ marginLeft: "auto" }}>{extra}</span>}
      </header>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fill, minmax(min(100%, 280px), 1fr))",
          gap: 12,
        }}
      >
        {children}
      </div>
    </section>
  );
}
