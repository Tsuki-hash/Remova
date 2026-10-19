// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { useRequestSeq } from "../hooks/useRequestSeq";

afterEach(cleanup);

// CQ-16: this hook replaces four hand-rolled copies of the same guard. The
// contract that matters is that a superseded request can never publish.
it("lets only the newest request publish and retires every slot on invalidate", () => {
  const { result } = renderHook(() => useRequestSeq());
  let first = 0;
  let second = 0;
  act(() => { first = result.current.next(); });
  act(() => { second = result.current.next(); });
  expect(second).toBeGreaterThan(first);
  expect(result.current.isCurrent(first)).toBe(false);
  expect(result.current.isCurrent(second)).toBe(true);

  act(() => { result.current.invalidate(); });
  expect(result.current.isCurrent(second)).toBe(false);

  let third = 0;
  act(() => { third = result.current.next(); });
  expect(result.current.isCurrent(third)).toBe(true);
});

it("keeps stable identities so it can sit in a dependency array", () => {
  const { result, rerender } = renderHook(() => useRequestSeq());
  const before = result.current;
  rerender();
  expect(result.current.next).toBe(before.next);
  expect(result.current.isCurrent).toBe(before.isCurrent);
  expect(result.current.invalidate).toBe(before.invalidate);
});
