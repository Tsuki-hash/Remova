/** Compare dotted semver-ish tags. Returns >0 if a newer than b.
 * A prerelease suffix (-rc1) sorts BEFORE the same release, and one leading
 * `v`/`V` is stripped — the same single-v rule the backend applies. */
export function compareSemver(a: string, b: string): number {
  const parse = (raw: string): { core: number[]; pre: string[] | null } => {
    const s = raw.trim().replace(/^[vV]/, "");
    const m = /^(\d+(?:\.\d+)*)(-[^+]*)?(\+.*)?$/.exec(s);
    if (!m) return { core: [0], pre: null };
    return {
      core: (m[1] ?? "0").split(".").map((x) => parseInt(x, 10) || 0),
      pre: m[2] ? m[2].slice(1).split(".") : null,
    };
  };
  const pa = parse(a);
  const pb = parse(b);
  for (let i = 0; i < Math.max(pa.core.length, pb.core.length); i++) {
    const d = (pa.core[i] || 0) - (pb.core[i] || 0);
    if (d !== 0) return d;
  }
  if (pa.pre === null || pb.pre === null) {
    return pa.pre === pb.pre ? 0 : pa.pre === null ? 1 : -1;
  }
  for (let i = 0; i < Math.max(pa.pre.length, pb.pre.length); i++) {
    const x = pa.pre[i], y = pb.pre[i];
    if (x === y) continue;
    if (x === undefined) return -1;
    if (y === undefined) return 1;
    const nx = /^\d+$/.test(x), ny = /^\d+$/.test(y);
    if (nx && ny) {
      const d = BigInt(x) - BigInt(y);
      if (d !== 0n) return d > 0n ? 1 : -1;
    } else if (nx !== ny) return nx ? -1 : 1;
    else return x < y ? -1 : 1;
  }
  return 0;
}
