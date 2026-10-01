/** Compare dotted semver-ish tags. Returns >0 if a newer than b.
 * A prerelease suffix (-rc1) sorts BEFORE the same release, and one leading
 * `v`/`V` is stripped — the same single-v rule the backend applies. */
export function compareSemver(a: string, b: string): number {
  const parse = (raw: string): { core: number[]; pre: boolean } => {
    const s = raw.trim().replace(/^[vV]/, "");
    const m = /^(\d+(?:\.\d+)*)(-[^+]*)?(\+.*)?$/.exec(s);
    if (!m) return { core: [0], pre: false };
    return {
      core: (m[1] ?? "0").split(".").map((x) => parseInt(x, 10) || 0),
      pre: m[2] !== undefined,
    };
  };
  const pa = parse(a);
  const pb = parse(b);
  for (let i = 0; i < Math.max(pa.core.length, pb.core.length); i++) {
    const d = (pa.core[i] || 0) - (pb.core[i] || 0);
    if (d !== 0) return d;
  }
  if (pa.pre !== pb.pre) return pa.pre ? -1 : 1;
  return 0;
}
