import { useLayoutEffect, type RefObject } from "react";

/** Bound a list to the space below its actual toolbar, without wasting the viewport. */
export function useListViewportHeight(ref: RefObject<HTMLDivElement | null>) {
  useLayoutEffect(() => {
    const list = ref.current;
    const main = list?.closest("main");
    if (!list || !main) return;
    let frame = 0;
    // PERF-7: `paddingBottom` was read through `getComputedStyle` on every
    // observation, which forces a style recalculation + reflow. It only changes
    // when the layout changes, so it is refreshed on resize instead of per call.
    let bottomPadding = Number.parseFloat(getComputedStyle(main).paddingBottom) || 0;
    const measure = () => {
      const top = Math.max(0, list.getBoundingClientRect().top - main.getBoundingClientRect().top);
      const height = `${Math.max(160, Math.floor(main.clientHeight - top - bottomPadding))}px`;
      if (list.style.getPropertyValue("--remova-list-height") !== height) {
        list.style.setProperty("--remova-list-height", height);
      }
    };
    const schedule = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(measure);
    };
    measure();
    main.addEventListener("scroll", schedule, { passive: true });
    main.addEventListener("toggle", schedule, { capture: true, passive: true });
    const resize = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => {
      bottomPadding = Number.parseFloat(getComputedStyle(main).paddingBottom) || 0;
      schedule();
    });
    resize?.observe(main);
    const content = new MutationObserver((records) => {
      // PERF-7: `characterData: true` fired on every keystroke anywhere under
      // `main`, and `childList` + `subtree` fired on every row the virtualizer
      // swapped in during a scroll — each callback ran `getComputedStyle`, so
      // scrolling forced a reflow per frame. Text cannot move the toolbar, and
      // the list rewriting its own rows cannot change the space above it.
      if (records.every((record) => list.contains(record.target))) return;
      schedule();
    });
    content.observe(main, { childList: true, subtree: true });
    return () => {
      resize?.disconnect();
      content.disconnect();
      main.removeEventListener("scroll", schedule);
      main.removeEventListener("toggle", schedule, { capture: true });
      cancelAnimationFrame(frame);
    };
  });
}
