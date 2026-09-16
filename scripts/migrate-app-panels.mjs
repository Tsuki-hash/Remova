/**
 * Safe one-shot App.tsx migration (CRLF-aware).
 * Fails hard if any anchor is missing or non-unique.
 */
import fs from "node:fs";

const p = "src/App.tsx";
let s = fs.readFileSync(p, "utf8");
const NL = "\r\n";
const before = s.length;

function mustReplace(label, oldStr, newStr) {
  const n = s.split(oldStr).length - 1;
  if (n === 0) throw new Error(`anchor missing: ${label}`);
  if (n !== 1) throw new Error(`anchor not unique (${n}): ${label}`);
  s = s.replace(oldStr, newStr);
  console.log("ok", label);
}

// 1) Imports
mustReplace(
  "imports",
  `import { compareSemver } from "./semver";${NL}`,
  [
    'import { compareSemver } from "./semver";',
    'import { cssStyles, globalCss } from "./styles";',
    'import { AppIcon } from "./components/AppIcon";',
    'import { HistoryPanel } from "./components/HistoryPanel";',
    'import { RestorePanel } from "./components/RestorePanel";',
    'import { MonitorPanel } from "./components/MonitorPanel";',
    'import { ManagePanel, type ManageItem, type ManageTab } from "./components/ManagePanel";',
    "import {",
    "  BatchProgress,",
    "  BatchSummaryPanel,",
    "  type BatchItemResult,",
    '} from "./components/BatchPanels";',
    "import {",
    "  escapeHtml,",
    "  formatError,",
    "  prettyAppName,",
    "  prettyPublisher,",
    "  shortPath,",
    '} from "./lib/format";',
    'import { applyTheme, loadTheme, type Theme } from "./lib/theme";',
    "",
    "declare const __APP_VERSION__: string;",
    "",
  ].join(NL),
);

// 2) Drop local Theme type + stale comment
mustReplace(
  "local Theme type",
  `type Theme = "light" | "dark";${NL}${NL}/** Module-level styles — not rebuilt every render (PERF-3). */${NL}`,
  "",
);

// 3) Drop local cssStyles + globalCss
const cssStart = s.indexOf("const cssStyles = {");
const iconStart = s.indexOf("/** Process-wide icon cache");
if (cssStart < 0 || iconStart < 0 || iconStart < cssStart) {
  throw new Error("cssStyles/globalCss block not found");
}
s = s.slice(0, cssStart) + s.slice(iconStart);
console.log("ok drop cssStyles+globalCss");

// 4) Drop local icon/theme/format helpers
const exportIdx = s.indexOf("export default function App()");
const iconCacheIdx = s.indexOf("/** Process-wide icon cache");
if (exportIdx < 0 || iconCacheIdx < 0 || iconCacheIdx > exportIdx) {
  throw new Error("helper block not found before App");
}
s = s.slice(0, iconCacheIdx) + s.slice(exportIdx);
console.log("ok drop local helpers");

// 5) Drop local type aliases
s = s.replaceAll(
  `${NL}  type BatchStatus = "ok" | "failed" | "skipped";${NL}  type BatchItemResult = {${NL}    key: string;${NL}    name: string;${NL}    status: BatchStatus;${NL}    detail: string;${NL}  };${NL}`,
  NL,
);
s = s.replaceAll(
  `${NL}  type ManageTab = "startup" | "services" | "tasks";${NL}  type ManageItem = {${NL}    name: string;${NL}    detail: string;${NL}    location: string;${NL}    enabled: boolean;${NL}  };${NL}`,
  NL,
);
console.log("ok drop local type aliases");

// 6) History panel
const histOld = s.slice(
  s.indexOf("      {showHistory && ("),
  s.indexOf("      {showRestore && ("),
);
if (!histOld.includes("historyTitle")) throw new Error("history block unexpected");
s = s.replace(
  histOld,
  [
    "      {showHistory && (",
    "        <HistoryPanel",
    "          history={history}",
    "          histQ={histQ}",
    "          setHistQ={setHistQ}",
    "          onClose={() => setShowHistory(false)}",
    "        />",
    "      )}",
    "",
    "",
  ].join(NL),
);
console.log("ok HistoryPanel");

// 7) Restore panel
const restOld = s.slice(
  s.indexOf("      {showRestore && ("),
  s.indexOf("      {monitorDiff && ("),
);
if (!restOld.includes("restoreSessions")) throw new Error("restore block unexpected");
s = s.replace(
  restOld,
  [
    "      {showRestore && (",
    "        <RestorePanel",
    "          sessions={restoreSessions}",
    "          pick={restorePick}",
    "          setPick={setRestorePick}",
    "          busy={restoreBusy}",
    "          msgs={restoreMsgs}",
    "          onRun={() => void runRestoreSession()}",
    "          onClose={() => setShowRestore(false)}",
    "        />",
    "      )}",
    "",
    "",
  ].join(NL),
);
console.log("ok RestorePanel");

// 8) Monitor panel
const monOld = s.slice(
  s.indexOf("      {monitorDiff && ("),
  s.indexOf("      {showManage && ("),
);
if (!monOld.includes("monitorDiff.added_files")) throw new Error("monitor block unexpected");
s = s.replace(
  monOld,
  [
    "      {monitorDiff && (",
    "        <MonitorPanel diff={monitorDiff} onDismiss={() => setMonitorDiff(null)} />",
    "      )}",
    "",
    "",
  ].join(NL),
);
console.log("ok MonitorPanel");

fs.writeFileSync(p, s);
console.log("done", { before, after: s.length, lines: s.split(NL).length });
