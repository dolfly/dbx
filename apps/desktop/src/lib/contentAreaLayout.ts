import type { QueryTab } from "@/types/database";

type QueryOutputPaneState = Pick<
  QueryTab,
  "mode" | "objectSource" | "result" | "explainPlan" | "explainError" | "isExecuting" | "isExplaining"
>;

export function shouldShowQueryOutputPane(tab: QueryOutputPaneState): boolean {
  if (tab.mode !== "query") return false;
  if (!tab.objectSource) return true;

  return !!(tab.result || tab.explainPlan || tab.explainError || tab.isExecuting || tab.isExplaining);
}
