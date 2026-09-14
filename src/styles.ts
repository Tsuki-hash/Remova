/** Module-level styles — not rebuilt every render (PERF-3 / ARCH-4). */

export const cssStyles = {
  page: {
    padding: "20px 24px 32px",
    maxWidth: 1400,
    margin: "0 auto",
    color: "var(--fg)",
    fontFamily:
      "'Segoe UI', 'PingFang SC', 'Microsoft YaHei UI', system-ui, sans-serif",
  },
  muted: { color: "var(--muted)", fontSize: 13 },
  input: {
    flex: 1,
    minWidth: 200,
    height: 40,
    borderRadius: 10,
    border: "1px solid var(--border)",
    padding: "0 14px",
    fontSize: 14,
    background: "var(--surface)",
    color: "var(--fg)",
    outline: "none",
  },
  btn: {
    height: 36,
    padding: "0 14px",
    borderRadius: 10,
    border: "none",
    background: "var(--accent)",
    color: "var(--accent-ink)",
    fontWeight: 600,
    fontSize: 13,
    cursor: "pointer",
    whiteSpace: "nowrap" as const,
    transition: "opacity .15s, transform .05s",
  },
  btnGhost: {
    height: 36,
    padding: "0 12px",
    borderRadius: 10,
    border: "1px solid var(--border)",
    background: "var(--surface)",
    color: "var(--fg)",
    fontSize: 13,
    cursor: "pointer",
    whiteSpace: "nowrap" as const,
    transition: "background .15s, border-color .15s, opacity .15s",
  },
  btnSm: {
    height: 30,
    padding: "0 10px",
    borderRadius: 8,
    border: "1px solid var(--border)",
    background: "var(--surface)",
    color: "var(--fg)",
    fontSize: 12,
    cursor: "pointer",
    whiteSpace: "nowrap" as const,
  },
  card: {
    background: "var(--surface)",
    border: "1px solid var(--border)",
    borderRadius: 14,
    overflow: "hidden",
    boxShadow: "var(--shadow)",
  },
  th: {
    textAlign: "left" as const,
    padding: "10px 12px",
    background: "var(--th-bg)",
    borderBottom: "1px solid var(--border)",
    position: "sticky" as const,
    top: 0,
    zIndex: 1,
    fontSize: 12,
    fontWeight: 600,
    color: "var(--muted)",
    letterSpacing: 0.2,
  },
  td: {
    padding: "10px 12px",
    borderBottom: "1px solid var(--border)",
    verticalAlign: "middle" as const,
    fontSize: 13,
  },
  table: { width: "100%", borderCollapse: "collapse" as const, fontSize: 13 },
  scroll: {
    maxHeight: "calc(100vh - 280px)",
    overflow: "auto" as const,
    overscrollBehavior: "contain" as const,
    position: "relative" as const,
  },
  toolbar: {
    display: "flex",
    gap: 8,
    alignItems: "center",
    flexWrap: "wrap" as const,
    marginBottom: 10,
  },
  chip: {
    display: "inline-flex",
    alignItems: "center",
    gap: 6,
    height: 28,
    padding: "0 10px",
    borderRadius: 999,
    background: "var(--surface-2)",
    border: "1px solid var(--border)",
    fontSize: 12,
    color: "var(--muted)",
  },
};

export const globalCss = `
  button:disabled { opacity: .45; cursor: not-allowed; }
  button:not(:disabled):hover { filter: brightness(1.05); }
  button:not(:disabled):active { transform: translateY(1px); }
  button:focus-visible, input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  input::placeholder { color: var(--muted); opacity: .85; }
  tbody tr { transition: background .12s; }
  tbody tr:hover { background: var(--row-hover); }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
`;
