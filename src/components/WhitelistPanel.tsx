import { useEffect, useState } from "react";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { api, type IgnoreLists } from "../lib/api";
import { formatError } from "../lib/format";
import { toast } from "../lib/toast";

/** App whitelist: list ignored publishers/names and remove them (undo). */
export function WhitelistPanel({
  onClose,
  onError,
  onIgnorePublisher,
}: {
  onClose: () => void;
  onError: (msg: string) => void;
  onIgnorePublisher?: () => void;
}) {
  const L = t();
  const [lists, setLists] = useState<IgnoreLists | null>(null);

  useEffect(() => {
    void api
      .loadIgnore()
      .then(setLists)
      .catch((e) => onError(formatError(e)));
  }, [onError]);

  const rows: { kind: "publisher" | "name"; value: string }[] = [
    ...(lists?.publishers ?? []).map((value) => ({ kind: "publisher" as const, value })),
    ...(lists?.names ?? []).map((value) => ({ kind: "name" as const, value })),
  ];

  return (
    <div
      style={{
        ...css.card,
        padding: "12px 14px",
        marginBottom: 18,
        fontSize: 13,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 10 }}>
        <strong style={{ fontSize: 13.5 }}>{L.appWhitelist}</strong>
        <span style={{ ...css.muted, fontSize: 12, flex: 1 }}>{L.appWhitelistHint}</span>
        {onIgnorePublisher && (
          <button
            style={{ ...css.btnSm, height: 28 }}
            onClick={() => {
              onIgnorePublisher();
              void api.loadIgnore().then(setLists).catch((e) => onError(formatError(e)));
            }}
          >
            {L.ignorePub}
          </button>
        )}
        <button style={{ ...css.btnGhost, height: 28 }} onClick={onClose}>
          ×
        </button>
      </div>
      {rows.length === 0 ? (
        <div style={{ color: "var(--muted)" }}>{L.ignoreSuggestNone || "—"}</div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          {rows.map((r) => (
            <div
              key={`${r.kind}:${r.value}`}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 10,
                padding: "8px 10px",
                border: "1px solid var(--border)",
                borderRadius: 8,
              }}
            >
              <span style={{ ...css.muted, fontSize: 11, minWidth: 56 }}>
                {r.kind === "publisher" ? L.ignorePub : L.chipNoUninstall}
              </span>
              <span style={{ flex: 1, minWidth: 0 }} className="ell">
                {r.value}
              </span>
              <button
                style={{ ...css.btnSm, height: 26 }}
                onClick={() => {
                  const p =
                    r.kind === "publisher"
                      ? api.unignorePublisher(r.value)
                      : api.unignoreAppName(r.value);
                  void p
                    .then((next) => {
                      setLists(next);
                      toast.success(L.ignoreSuggestDone);
                    })
                    .catch((e) => onError(formatError(e)));
                }}
              >
                {L.whitelistRemove}
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
