import { useId, useRef, useState, type KeyboardEvent, type RefObject } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { t } from "../i18n";
import type { ManageItem } from "../types";
import { AppIcon } from "./AppIcon";
import { ToolGlyph } from "./ToolIcons";

export function taskName(item: ManageItem) {
  return item.display_name || item.name.split("\\").filter(Boolean).pop() || item.name;
}

function taskProgram(item: ManageItem, L: ReturnType<typeof t>) {
  const action = item.task_action;
  if (action?.kind === "multiple") return L.manageTaskMultiple;
  if (action?.kind === "program") {
    return action.script_host ? L.manageTaskVia(action.program) : L.manageTaskRuns(action.program);
  }
  return L.manageTaskNoProgram;
}

function TaskIcon({ item }: { item: ManageItem }) {
  const action = item.task_action;
  const glyph = action?.kind === "multiple" ? "task-multiple"
    : action?.kind === "program" ? (action.script_host ? "task-command" : "package") : "tasks";
  return (
    <AppIcon displayIcon={item.display_icon || ""} name={taskName(item)} embeddedOnly
      bare fallback={<ToolGlyph name={glyph} size={22} />} />
  );
}

export function TaskWorkspace({ rows, scrollRef, busy, emptyLabel, onSetEnabled }: {
  rows: ManageItem[];
  scrollRef: RefObject<HTMLDivElement | null>;
  busy: boolean;
  emptyLabel?: string;
  onSetEnabled: (item: ManageItem, enabled: boolean) => Promise<void>;
}) {
  const L = t();
  const listRef = useRef<HTMLDivElement>(null);
  const [selection, setSelection] = useState<string | null>(null);
  const [changing, setChanging] = useState(false);
  const changingRef = useRef(false);
  const listId = useId();
  const selectedIndex = selection === null ? 0 : rows.findIndex((item) => item.name === selection);
  const selected = rows[selectedIndex];
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => listRef.current,
    estimateSize: () => 62,
    getItemKey: (index) => rows[index]?.name ?? index,
    overscan: 10,
  });
  const visible = virtualizer.getVirtualItems();
  const enabledLabel = (item: ManageItem) => item.enabled ? L.manageStatusEnabled : L.manageStatusDisabled;

  function moveSelection(event: KeyboardEvent<HTMLDivElement>) {
    const next = event.key === "Home" ? 0
      : event.key === "End" ? rows.length - 1
        : event.key === "ArrowUp" ? Math.max(0, selectedIndex - 1)
          : event.key === "ArrowDown" ? Math.min(rows.length - 1, selectedIndex + 1) : null;
    if (next === null || !rows[next]) return;
    event.preventDefault();
    setSelection(rows[next].name);
    virtualizer.scrollToIndex(next, { align: "auto" });
  }

  async function changeEnabled(item: ManageItem) {
    if (busy || changingRef.current) return;
    changingRef.current = true;
    setChanging(true);
    try {
      await onSetEnabled(item, !item.enabled);
    } finally {
      changingRef.current = false;
      setChanging(false);
    }
  }

  return (
    <div className="remova-task-workspace" ref={scrollRef}>
      <section className="remova-task-list">
        <div className="remova-task-columns" aria-hidden>
          <span>{L.manageTaskName}</span>
          <span>{L.manageTaskStatus}</span>
        </div>
        <div
          ref={listRef}
          className="remova-task-scroll"
          role="listbox"
          aria-label={L.navTasks}
          aria-busy={busy}
          aria-activedescendant={visible.some((row) => row.index === selectedIndex) ? `${listId}-${selectedIndex}` : undefined}
          tabIndex={0}
          onKeyDown={moveSelection}
        >
          <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
            {visible.map(({ index, start, size, key }) => {
              const item = rows[index];
              if (!item) return null;
              const name = taskName(item);
              const summary = item.detail || taskProgram(item, L);
              return (
                <div
                  id={`${listId}-${index}`}
                  key={key}
                  className="remova-manage-row remova-task-row"
                  role="option"
                  aria-selected={index === selectedIndex}
                  aria-posinset={index + 1}
                  aria-setsize={rows.length}
                  title={item.location}
                  style={{ height: size, transform: `translateY(${start}px)` }}
                  onClick={() => {
                    setSelection(item.name);
                    listRef.current?.focus({ preventScroll: true });
                  }}
                >
                  <TaskIcon item={item} />
                  <span className="remova-task-summary">
                    <strong className="ell">{name}</strong>
                    <span className="ell" title={summary}>{summary}</span>
                  </span>
                  <span className="remova-task-status" data-enabled={item.enabled}>{enabledLabel(item)}</span>
                </div>
              );
            })}
          </div>
        </div>
        {!rows.length && emptyLabel && <p className="remova-task-empty">{emptyLabel}</p>}
      </section>
      <aside className="remova-task-detail" aria-label={L.manageTaskDetails}>
        {selected ? (
          <>
            <header>
              <TaskIcon item={selected} />
              <div>
                <h2>{taskName(selected)}</h2>
                <span className="remova-task-status" data-enabled={selected.enabled}>{enabledLabel(selected)}</span>
              </div>
            </header>
            {selected.detail && <p className="remova-task-description">{selected.detail}</p>}
            <dl>
              <dt>{L.manageTaskPath}</dt>
              <dd className="remova-task-path">{selected.location}</dd>
              <dt>{L.manageTaskProgram}</dt>
              <dd>
                {selected.task_action?.kind === "program" && !selected.task_action.script_host
                  ? selected.task_action.program : taskProgram(selected, L)}
                {selected.path && selected.path !== (selected.task_action?.kind === "program" && selected.task_action.program) && (
                  <span className="remova-task-binary remova-task-path">{selected.path}</span>
                )}
              </dd>
              <dt>{L.manageLastRun}</dt>
              <dd>{selected.last_run || L.manageNoLastRun}</dd>
              <dt>{L.manageNextRun}</dt>
              <dd>{selected.next_run || L.manageNoLastRun}</dd>
            </dl>
            <button
              type="button"
              className="remova-task-action"
              aria-label={`${selected.enabled ? L.manageTaskDisable : L.manageTaskEnable}: ${taskName(selected)}`}
              aria-disabled={busy || changing}
              onClick={() => void changeEnabled(selected)}
            >
              {selected.enabled ? L.manageTaskDisable : L.manageTaskEnable}
            </button>
          </>
        ) : (
          <p className="remova-task-empty">{busy ? L.loadingManage : L.manageTaskSelect}</p>
        )}
      </aside>
    </div>
  );
}
