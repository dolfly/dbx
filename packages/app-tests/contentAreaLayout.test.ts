import { strict as assert } from "node:assert";
import test from "node:test";
import { shouldShowQueryOutputPane } from "../../apps/desktop/src/lib/contentAreaLayout.ts";

test("object source tabs hide the query output pane while idle", () => {
  assert.equal(
    shouldShowQueryOutputPane({
      mode: "query",
      objectSource: { objectType: "PROCEDURE", name: "refresh_cache" },
      isExecuting: false,
      isExplaining: false,
    }),
    false,
  );
});

test("object source tabs show the query output pane after execution starts or returns feedback", () => {
  assert.equal(
    shouldShowQueryOutputPane({
      mode: "query",
      objectSource: { objectType: "FUNCTION", name: "calc_total" },
      isExecuting: true,
      isExplaining: false,
    }),
    true,
  );

  assert.equal(
    shouldShowQueryOutputPane({
      mode: "query",
      objectSource: { objectType: "PROCEDURE", name: "refresh_cache" },
      isExecuting: false,
      isExplaining: false,
      result: {
        columns: ["Error"],
        rows: [["syntax error"]],
        affected_rows: 0,
        execution_time_ms: 2,
      },
    }),
    true,
  );
});

test("regular query tabs keep the output pane visible before execution", () => {
  assert.equal(
    shouldShowQueryOutputPane({
      mode: "query",
      isExecuting: false,
      isExplaining: false,
    }),
    true,
  );
});
