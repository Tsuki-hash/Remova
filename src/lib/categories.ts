/** Single source for software-list category ids. */
export type CategoryId = "all" | "desktop" | "store" | "large" | "recent";
export type SortCol = "name" | "size" | "recommend" | null;

export const CATEGORY_IDS: CategoryId[] = ["all", "desktop", "store", "large", "recent"];
