import { useLayoutEffect, type RefObject } from "react";

/** Bound a list to the space below its actual toolbar, without wasting the viewport. */
export function useListViewportHeight(ref: RefObject<HTMLDivElement | null>) {
  useLayoutEffect(() => {
    const list = ref.current;
    const main = list?.closest("main");
    if (!list || !main) return;
    let frame = 0;
    const measure = () => {
      const top = Math.max(0, list.getBoundingClientRect().top - main.getBoundingClientRect().top);
      const bottomPadding = Number.parseFloat(getComputedStyle(main).paddingBottom) || 0;
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
    main.addEventListener("toggle", schedule, true);
    const resize = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(schedule);
    resize?.observe(main);
    const content = new MutationObserver(schedule);
    content.observe(main, { childList: true, subtree: true, characterData: true });
    return () => {
      resize?.disconnect();
      content.disconnect();
      main.removeEventListener("scroll", schedule);
      main.removeEventListener("toggle", schedule, true);
      cancelAnimationFrame(frame);
    };
  });
}
