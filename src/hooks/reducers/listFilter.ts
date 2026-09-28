import type { CategoryId, SortCol } from "../../lib/categories";

export type { CategoryId };
export type SortColState = SortCol;

export type ListFilterState = {
  q: string;
  sortCol: SortColState;
  sortDesc: boolean;
  category: CategoryId;
};

export type ListFilterAction =
  | { type: "q/set"; value: string }
  | { type: "sortCol/set"; value: SortColState }
  | { type: "sortDesc/set"; value: boolean }
  | { type: "sortDesc/update"; value: (d: boolean) => boolean }
  | { type: "category/set"; value: CategoryId };

function loadCategory(): CategoryId {
  const v = localStorage.getItem("remova_cat");
  return v === "desktop" || v === "store" || v === "large" || v === "recent" ? v : "all";
}

export function initialListFilterState(): ListFilterState {
  return { q: "", sortCol: null, sortDesc: false, category: loadCategory() };
}

export function listFilterReducer(state: ListFilterState, action: ListFilterAction): ListFilterState {
  switch (action.type) {
    case "q/set":
      return { ...state, q: action.value };
    case "sortCol/set":
      return { ...state, sortCol: action.value };
    case "sortDesc/set":
      return { ...state, sortDesc: action.value };
    case "sortDesc/update":
      // REV-FE-03: functional update — never compute from a stale closure snapshot.
      return { ...state, sortDesc: action.value(state.sortDesc) };
    case "category/set":
      return { ...state, category: action.value };
    default:
      return state;
  }
}
