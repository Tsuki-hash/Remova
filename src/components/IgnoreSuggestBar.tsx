import { api } from "../lib/api";
import { CloseGlyph } from "./ui/Glyph";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { toast } from "../lib/toast";
import type { IgnoreSuggestion } from "../types";

export function IgnoreSuggestBar({
  suggestions,
  onApplied,
  onDismiss,
}: {
  suggestions: IgnoreSuggestion[];
  onApplied: (p: string[], n: string[]) => void;
  onDismiss: () => void;
}) {
  const L = t();
  if (!suggestions.length) return null;
  return (
    <div
      style={{
        marginBottom: 8,
        padding: "8px 12px",
        borderRadius: 8,
        border: "1px solid var(--border)",
        background: "var(--surface-2)",
        fontSize: 12.5,
        display: "flex",
        gap: 10,
        flexWrap: "wrap",
        alignItems: "center",
        flexShrink: 0,
      }}
    >
      <strong>{L.ignoreSuggestTitle}</strong>
      <span style={{ color: "var(--muted)", flex: "1 1 180px" }}>
        {suggestions.map((s) => `${s.value} (${s.reason})`).join("; ")}
      </span>
      <button
        style={css.btnSm}
        onClick={() => {
          void (async () => {
            try {
              const ig = await api.applyIgnoreSuggestions(suggestions);
              onApplied(ig.publishers || [], ig.names || []);
              toast.success(L.ignoreSuggestDone);
            } catch (e) {
              toast.error(formatError(e));
            }
          })();
        }}
      >
        {L.ignoreSuggestApply}
      </button>
      <button style={css.btnSm} onClick={onDismiss}>
        <CloseGlyph />
      </button>
    </div>
  );
}
