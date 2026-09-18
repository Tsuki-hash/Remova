import { useMemo } from "react";
import type { SoftwarePageProps } from "../components/SoftwarePage";

/**
 * Identity-stable SoftwarePage props (A-04 / P-02).
 * Caller builds the arg bag in useMemo; this pass-through keeps a single
 * controller type for SoftwarePage's React.memo comparison.
 */
export function useSoftwareController(args: SoftwarePageProps): SoftwarePageProps {
  return useMemo(() => args, [args]);
}

export type { SoftwarePageProps };
