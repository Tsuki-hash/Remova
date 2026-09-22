import { useCallback, useReducer } from "react";
import type { CategoryId } from "./reducers/listFilter";
import { initialListFilterState, listFilterReducer } from "./reducers/listFilter";

/** List filter chrome (q / sort / category) — domain reducer. */
export function useListFilterChrome() {
  const [state, dispatch] = useReducer(listFilterReducer, undefined, initialListFilterState);

  const setQ = useCallback((value: string) => dispatch({ type: "q/set", value }), []);
  const setSortCol = useCallback(
    (value: "name" | "size" | "recommend" | null) => dispatch({ type: "sortCol/set", value }),
    [],
  );
  const setSortDesc = useCallback(
    (value: boolean | ((d: boolean) => boolean)) => {
      if (typeof value === "function") {
        dispatch({ type: "sortDesc/set", value: value(state.sortDesc) });
      } else {
        dispatch({ type: "sortDesc/set", value });
      }
    },
    [state.sortDesc],
  );
  const setCategoryState = useCallback((value: CategoryId) => {
    dispatch({ type: "category/set", value });
  }, []);
  const setQuery = useCallback((value: string) => dispatch({ type: "q/set", value }), []);

  return {
    q: state.q,
    setQ,
    sortCol: state.sortCol,
    setSortCol,
    sortDesc: state.sortDesc,
    setSortDesc,
    category: state.category,
    setCategoryState,
    actions: { setQuery, setCategory: setCategoryState },
  };
}
