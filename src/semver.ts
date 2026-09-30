/** Compare dotted semver-ish tags. Returns >0 if a newer than b. */
export function compareSemver(a: string, b: string): number {
  // Strip exactly one leading `v` so a raw git tag never compares as 0.0.0 —
  // the same single-v rule the backend applies to release tags.
  const strip = (s: string) => (s.startsWith("v") ? s.slice(1) : s);
  const pa = strip(a).split(".").map((x) => parseInt(x, 10) || 0);
  const pb = strip(b).split(".").map((x) => parseInt(x, 10) || 0);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] || 0) - (pb[i] || 0);
    if (d !== 0) return d;
  }
  return 0;
}
