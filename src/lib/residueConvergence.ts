import type { CleanupItem } from "../types";

/**
 * Residue-convergence comparator for the post-uninstall review rounds.
 *
 * "Converged" is a claim about evidence, not a guess: two complete rounds
 * whose residue identities match are converged; any incomplete round, or a
 * round pair whose sets differ, keeps the review open. One round can never
 * converge by itself — a single scan cannot prove the second sweep would not
 * surface more.
 */

export type ConvergenceRound = {
  items: CleanupItem[];
  /** False when the scan backing this round was partial/cancelled/failed. */
  complete: boolean;
};

export type ResidueConvergence = {
  /** true = rounds agree and every round was complete; false = sets differ;
   *  null = cannot judge yet (fewer than two rounds, or an incomplete round). */
  converged: boolean | null;
  /** Rounds recorded so far. */
  rounds: number;
  /** Residue identities still present in the latest round. */
  remaining: string[];
  /** Identities that appeared in the latest round but were absent before. */
  appeared: string[];
  /** Identities present in an earlier round but gone from the latest. */
  disappeared: string[];
  /** How many of the recorded rounds were incomplete. */
  incompleteRounds: number;
};

/** Stable identity of one residue candidate across rounds (kind + path). */
export function residueKey(it: CleanupItem): string {
  return `${it.kind}:${it.path}`;
}

function keySet(items: CleanupItem[]): Set<string> {
  return new Set(items.map(residueKey));
}

export function compareConvergence(rounds: ConvergenceRound[]): ResidueConvergence {
  const incompleteRounds = rounds.filter((r) => !r.complete).length;
  const last = rounds.at(-1);
  const remaining = last ? [...keySet(last.items)].sort() : [];
  if (rounds.length < 2 || incompleteRounds > 0) {
    return {
      converged: null,
      rounds: rounds.length,
      remaining,
      appeared: [],
      disappeared: [],
      incompleteRounds,
    };
  }
  const prev = keySet(rounds[rounds.length - 2]!.items);
  const curr = keySet(last!.items);
  const appeared: string[] = [];
  const disappeared: string[] = [];
  for (const key of curr) if (!prev.has(key)) appeared.push(key);
  for (const key of prev) if (!curr.has(key)) disappeared.push(key);
  appeared.sort();
  disappeared.sort();
  return {
    converged: appeared.length === 0 && disappeared.length === 0,
    rounds: rounds.length,
    remaining,
    appeared,
    disappeared,
    incompleteRounds,
  };
}
