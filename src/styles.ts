/**
 * Remova styles — restrained Windows tool chrome.
 * Tokens live in lib/theme.ts. Minimal radius/shadow; blue only for interaction.
 */

export const cssStyles = {
  page: {
    padding: "12px 14px 12px",
    maxWidth: "100%",
    height: "100vh",
    display: "flex",
    flexDirection: "column" as const,
    color: "var(--fg)",
    fontFamily:
      "'Segoe UI Variable Text', 'Segoe UI', 'PingFang SC', 'Microsoft YaHei UI', system-ui, sans-serif",
    fontSize: 13,
    lineHeight: 1.45,
    overflow: "hidden",
  },
  muted: { color: "var(--muted)", fontSize: 12 },
  mono: {
    fontFamily: "var(--mono)",
    fontSize: 12,
    fontVariantNumeric: "tabular-nums" as const,
  },
  input: {
    flex: 1,
    minWidth: 200,
    height: 36,
    borderRadius: 8,
    border: "1px solid var(--border)",
    padding: "0 12px",
    fontSize: 13,
    background: "var(--surface)",
    color: "var(--fg)",
    outline: "none",
    transition: "border-color .12s",
  },
  btn: {
    height: 36,
    padding: "0 16px",
    borderRadius: 8,
    border: "1px solid transparent",
    background: "var(--accent)",
    color: "var(--accent-ink)",
    fontWeight: 600,
    fontSize: 13,
    letterSpacing: 0.1,
    cursor: "pointer",
    whiteSpace: "nowrap" as const,
    transition: "filter .12s, transform .05s",
  },
  btnGhost: {
    height: 34,
    padding: "0 12px",
    borderRadius: 6,
    border: "1px solid var(--border)",
    background: "transparent",
    color: "var(--fg)",
    fontSize: 12.5,
    cursor: "pointer",
    whiteSpace: "nowrap" as const,
    transition: "background .12s, border-color .12s, color .12s",
  },
  btnSm: {
    height: 30,
    padding: "0 12px",
    borderRadius: 6,
    border: "1px solid var(--border)",
    background: "transparent",
    color: "var(--muted)",
    fontSize: 12,
    cursor: "pointer",
    whiteSpace: "nowrap" as const,
    transition: "background .12s, color .12s, border-color .12s",
  },
  btnDanger: {
    height: 34,
    padding: "0 14px",
    borderRadius: 6,
    border: "1px solid transparent",
    background: "var(--danger)",
    color: "#1a0505",
    fontWeight: 600,
    fontSize: 12.5,
    cursor: "pointer",
    whiteSpace: "nowrap" as const,
  },
  card: {
    background: "var(--surface)",
    border: "1px solid var(--border)",
    borderRadius: 8,
    overflow: "hidden",
    boxShadow: "var(--shadow)",
  },
  th: {
    textAlign: "left" as const,
    padding: "10px 10px",
    background: "var(--th-bg)",
    borderBottom: "1px solid var(--border)",
    position: "sticky" as const,
    top: 0,
    zIndex: 1,
    fontSize: 11,
    fontWeight: 600,
    color: "var(--muted)",
    letterSpacing: 0.4,
    textTransform: "uppercase" as const,
    whiteSpace: "nowrap" as const,
    overflow: "hidden",
    textOverflow: "ellipsis",
  },
  td: {
    padding: "10px 10px",
    borderBottom: "1px solid var(--border)",
    verticalAlign: "middle" as const,
    fontSize: 13,
    overflow: "hidden",
  },
  table: {
    width: "100%",
    borderCollapse: "collapse" as const,
    fontSize: 13,
    tableLayout: "fixed" as const,
  },
  scroll: {
    flex: 1,
    minHeight: 0,
    overflow: "auto" as const,
    overscrollBehavior: "contain" as const,
    position: "relative" as const,
  },
  toolbar: {
    display: "flex",
    gap: 6,
    alignItems: "center",
    flexWrap: "wrap" as const,
    marginBottom: 8,
    flexShrink: 0,
  },
  chip: {
    display: "inline-flex",
    alignItems: "center",
    gap: 6,
    height: 24,
    padding: "0 9px",
    borderRadius: 999,
    background: "var(--surface-2)",
    border: "1px solid var(--border)",
    fontSize: 11,
    color: "var(--muted)",
    fontFamily: "var(--mono)",
    letterSpacing: 0.2,
  },
  chipAccent: {
    display: "inline-flex",
    alignItems: "center",
    gap: 6,
    height: 24,
    padding: "0 9px",
    borderRadius: 999,
    background: "var(--accent-soft)",
    border: "1px solid transparent",
    fontSize: 11,
    color: "var(--accent)",
    fontWeight: 600,
  },
  chipDanger: {
    display: "inline-flex",
    alignItems: "center",
    gap: 6,
    height: 24,
    padding: "0 9px",
    borderRadius: 999,
    background: "var(--danger-soft)",
    border: "1px solid transparent",
    fontSize: 11,
    color: "var(--danger)",
    fontWeight: 600,
  },
  sourceBadge: {
    display: "inline-block",
    padding: "1px 7px",
    borderRadius: 3,
    fontSize: 10.5,
    fontWeight: 600,
    letterSpacing: 0.4,
    fontFamily: "var(--mono)",
    background: "var(--surface-2)",
    color: "var(--muted)",
    border: "1px solid var(--border)",
  },
  /** Right detail / panel chrome (1.1 style tokens). */
  panelShell: {
    width: 340,
    flexShrink: 0,
    background: "var(--surface)",
    border: "1px solid var(--border)",
    borderRadius: 8,
    display: "flex",
    flexDirection: "column" as const,
    minHeight: 0,
    overflow: "hidden",
  },
  sectionTitle: {
    fontWeight: 700,
    fontSize: 13,
    marginBottom: 8,
  },
  detailRow: {
    display: "flex",
    gap: 10,
    alignItems: "center",
    padding: "7px 0",
    borderBottom: "1px solid var(--border)",
    fontSize: 12.5,
  },
  panelHeader: {
    display: "flex",
    gap: 12,
    alignItems: "flex-start",
    padding: "14px 14px 10px",
    borderBottom: "1px solid var(--border)",
  },
  panelBody: {
    flex: 1,
    minHeight: 0,
    overflow: "auto",
    padding: "12px 14px",
  },
};

export const globalCss = `
  * { box-sizing: border-box; }
  html, body, #root { height: 100%; }
  body {
    margin: 0;
    background: var(--bg);
    color: var(--fg);
    -webkit-font-smoothing: antialiased;
  }
  button:disabled { opacity: .4; cursor: not-allowed; }
  button:not(:disabled):hover { filter: brightness(1.08); }
  button:not(:disabled):active { transform: translateY(0.5px); }
  button:focus-visible, input:focus-visible, select:focus-visible, th[tabindex]:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  input::placeholder { color: var(--muted); opacity: .7; }
  input:hover, select:hover { border-color: var(--border-strong); }
  tbody tr { transition: background .1s; }
  tbody tr:hover { background: var(--row-hover); }
  tbody tr .remova-row-uninstall {
    color: var(--accent);
    border-color: var(--border) !important;
    background: var(--surface) !important;
  }
  tbody tr:hover .remova-row-uninstall:not(:disabled) {
    color: var(--accent);
    background: var(--accent-soft) !important;
    border-color: var(--accent) !important;
  }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  ::selection { background: var(--accent-soft); color: var(--fg); }
  /* Slim scrollbars for the tool aesthetic */
  ::-webkit-scrollbar { width: 10px; height: 10px; }
  ::-webkit-scrollbar-track { background: transparent; }
  ::-webkit-scrollbar-thumb {
    background: var(--border-strong);
    border-radius: 5px;
    border: 2px solid var(--bg);
  }
  ::-webkit-scrollbar-thumb:hover { background: var(--muted); }
  @media (prefers-reduced-motion: reduce) {
    *, *::before, *::after { transition: none !important; animation: none !important; }
  }
  @keyframes remova-pulse {
    0%, 100% { opacity: .55; }
    50% { opacity: 1; }
  }
  .remova-skeleton {
    animation: remova-pulse 1.2s ease-in-out infinite;
    background: var(--surface-2);
    border-radius: 8px;
  }
  .remova-progress {
    height: 4px;
    border-radius: 999px;
    background: var(--accent-soft);
    overflow: hidden;
    position: relative;
  }
  .remova-progress::after {
    content: "";
    position: absolute;
    inset: 0;
    width: 40%;
    border-radius: inherit;
    background: var(--accent);
    animation: remova-indeterminate 1.1s ease-in-out infinite;
  }
  @keyframes remova-indeterminate {
    0% { transform: translateX(-100%); }
    100% { transform: translateX(250%); }
  }
`;
