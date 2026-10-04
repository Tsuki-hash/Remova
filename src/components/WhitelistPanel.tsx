import { useEffect, useRef, useState } from "react";
import { CloseGlyph } from "./ui/Glyph";
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
  onListsChange,
}: {
  onClose: () => void;
  onError: (msg: string) => void;
  onIgnorePublisher?: () => Promise<IgnoreLists | undefined>;
  onListsChange?: (lists: IgnoreLists) => void;
}) {
  const L = t();
  const [lists, setLists] = useState<IgnoreLists | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [retry, setRetry] = useState(0);
  const [pending, setPending] = useState(false);
  const pendingRef = useRef(false);

  const mutate = async (write: () => Promise<IgnoreLists | undefined>) => {
    if (pendingRef.current || lists === null) return;
    pendingRef.current = true;
    setPending(true);
    try {
      const next = await write();
      if (next) {
        setLists(next);
        onListsChange?.(next);
      }
    } catch (e) {
      onError(formatError(e));
    } finally {
      pendingRef.current = false;
      setPending(false);
    }
  };

  useEffect(() => {
    let alive = true;
    setLoadError(null);
    void api
      .loadIgnore()
      .then(next => { if (alive) setLists(next); })
      .catch((e) => {
        if (!alive) return;
        const message = formatError(e);
        setLoadError(message);
        onError(message);
      });
    return () => { alive = false; };
  }, [onError, retry]);

  const rows: { kind: "publisher" | "name"; value: string }[] = [
    ...(lists?.publishers ?? []).map((value) => ({ kind: "publisher" as const, value })),
    ...(lists?.names ?? []).map((value) => ({ kind: "name" as const, value })),
  ];

  return (
    <div
      style={{
        ...css.card,
        padding: "16px 18px",
        marginBottom: 18,
        fontSize: 13,
      }}
    >
      <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 10, marginBottom: 12 }}>
        <strong style={{ fontSize: 13, fontWeight: 600 }}>{L.appWhitelist}</strong>
        <span style={{ ...css.muted, fontSize: 12, flex: 1 }}>{L.appWhitelistHint}</span>
        {onIgnorePublisher && (
          <button
            style={{ ...css.btnSm, height: 28 }}
            disabled={pending || lists === null}
            onClick={() => void mutate(onIgnorePublisher)}
          >
            {L.ignorePub}
          </button>
        )}
        <button style={{ ...css.btnGhost, height: 28 }} onClick={onClose} aria-label={L.panelClose}>
          <CloseGlyph />
        </button>
      </div>
      {lists === null ? (
        loadError ? <div role="alert">
          <div style={{ color: "var(--danger-text)", marginBottom: 8 }}>{loadError}</div>
          <button style={css.btnSm} onClick={() => setRetry(value => value + 1)}>{L.manageReload}</button>
        </div> : <div style={{ color: "var(--muted)" }}>{L.loadingGeneric}</div>
      ) : rows.length === 0 ? (
        <div style={{ color: "var(--muted)" }}>{L.whitelistEmpty}</div>
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
                {r.kind === "publisher" ? L.ignorePub : L.ignoreApp}
              </span>
              <span style={{ flex: 1, minWidth: 0 }} className="ell" title={r.value}>
                {r.value}
              </span>
              <button
                style={{ ...css.btnSm, height: 26 }}
                disabled={pending}
                onClick={() => void mutate(async () => {
                  const next = await (
                    r.kind === "publisher"
                      ? api.unignorePublisher(r.value)
                      : api.unignoreAppName(r.value));
                  toast.success(L.ignoreSuggestDone);
                  return next;
                })}
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
