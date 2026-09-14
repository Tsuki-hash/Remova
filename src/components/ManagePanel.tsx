import { t } from "../i18n";
import { cssStyles as css } from "../styles";

export type ManageTab = "startup" | "services" | "tasks";
export type ManageItem = {
  name: string;
  detail: string;
  location: string;
  enabled: boolean;
};

export function ManagePanel({
  tab,
  items,
  busy,
  onTab,
  onReload,
  onToggle,
  onClose,
}: {
  tab: ManageTab;
  items: ManageItem[];
  busy: boolean;
  onTab: (tab: ManageTab) => void;
  onReload: () => void;
  onToggle: (item: ManageItem) => void;
  onClose: () => void;
}) {
  const L = t();
  return (
    <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
      <div style={{ display: "flex", gap: 8, marginBottom: 8, alignItems: "center" }}>
        <strong>{L.manage}</strong>
        {(["startup", "services", "tasks"] as ManageTab[]).map((tb) => (
          <button
            key={tb}
            style={{
              ...css.btnGhost,
              height: 30,
              borderColor: tab === tb ? "var(--accent)" : undefined,
            }}
            onClick={() => onTab(tb)}
          >
            {tb === "startup" ? L.manageStartup : tb === "services" ? L.manageServices : L.manageTasks}
          </button>
        ))}
        <button style={{ ...css.btnGhost, height: 30 }} onClick={onReload}>
          {L.manageReload}
        </button>
        <button style={{ ...css.btnGhost, height: 30, marginLeft: "auto" }} onClick={onClose}>
          ×
        </button>
      </div>
      <div style={{ maxHeight: 240, overflow: "auto" }}>
        {busy && <div style={css.muted}>{L.estimatingSizes}</div>}
        {!busy && items.length === 0 && <div style={css.muted}>{L.noHistory}</div>}
        {!busy &&
          items.slice(0, 200).map((it) => (
            <div
              key={it.location + it.name}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 10,
                padding: "6px 2px",
                borderBottom: "1px solid var(--border)",
              }}
            >
              <div style={{ flex: 1, minWidth: 0 }}>
                <div>{it.name}</div>
                <div style={{ ...css.muted, fontSize: 12, wordBreak: "break-all" }}>
                  {it.detail || it.location}
                </div>
              </div>
              <span style={{ color: it.enabled ? "var(--accent)" : "var(--muted)" }}>
                {it.enabled ? "●" : "○"}
              </span>
              <button
                style={{ ...css.btnGhost, height: 28 }}
                disabled={busy}
                onClick={() => onToggle(it)}
              >
                {it.enabled ? L.manageDisable : L.manageEnable}
              </button>
            </div>
          ))}
      </div>
    </div>
  );
}
