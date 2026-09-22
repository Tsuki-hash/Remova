import type { ReactNode } from "react";
import { cssStyles as css } from "../styles";

export function Section({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <section style={{ display: "flex", flexDirection: "column", gap: 10, marginBottom: 18 }}>
      <header
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: 10,
          paddingBottom: 6,
          borderBottom: "1px solid var(--border)",
        }}
      >
        <span style={{ fontWeight: 700, fontSize: 13.5, letterSpacing: 0.1 }}>{title}</span>
        {hint && <span style={{ ...css.muted, fontSize: 12 }}>{hint}</span>}
      </header>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fill, minmax(280px, 1fr))",
          gap: 10,
        }}
      >
        {children}
      </div>
    </section>
  );
}
