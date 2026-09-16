import fs from "node:fs";

const p = "src/App.tsx";
let s = fs.readFileSync(p, "utf8");

// Remove local Theme redeclare and stale style comment after imports.
s = s.replace(/\r?\ntype Theme = "light" \| "dark";\r?\n/, "\n");
s = s.replace(/\r?\n\/\*\* Module-level styles[\s\S]*?\*\/\r?\n/, "\n");

// Remove local inline type aliases if still present.
s = s.replace(
  /\r?\n  type BatchStatus = "ok" \| "failed" \| "skipped";\r?\n  type BatchItemResult = \{[\s\S]*?\};\r?\n/,
  "\n",
);
s = s.replace(
  /\r?\n  type ManageTab = "startup" \| "services" \| "tasks";\r?\n  type ManageItem = \{[\s\S]*?\};\r?\n/,
  "\n",
);

fs.writeFileSync(p, s);
console.log("lines", s.split(/\r?\n/).length);
