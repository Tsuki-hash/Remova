import fs from "node:fs";
const s = fs.readFileSync("src/App.tsx", "utf8");
console.log("len", s.length);
console.log("head", JSON.stringify(s.slice(0, 400)));
console.log("semver", s.includes('from "./semver"'));
console.log("crlf", s.includes("\r\n"));
