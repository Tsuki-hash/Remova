import { useCallback, useRef } from "react";

/**
 * one implementation of "a late async response must not publish".
 *
 * Five components hand-rolled the same three lines (`const seq = ++ref.current`
 * on entry, `if (seq !== ref.current) return` after each await, and
 * `ref.current += 1` on unmount). Four of them spelled it slightly differently,
 * so a reviewer could not answer "which guard does this panel actually use?".
 *
 * `next()` claims a slot, `isCurrent(token)` says whether that slot is still the
 * newest one, and `invalidate()` retires every slot (unmount / explicit reset).
 */
export function useRequestSeq() {
  const seq = useRef(0);
  const next = useCallback(() => ++seq.current, []);
  const isCurrent = useCallback((token: number) => token === seq.current, []);
  const invalidate = useCallback(() => {
    seq.current += 1;
  }, []);
  return { next, isCurrent, invalidate };
}
