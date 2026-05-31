import { strict as assert } from "node:assert";
import test from "node:test";
import { createPinia, setActivePinia } from "pinia";
import { useConnectionStore } from "../../apps/desktop/src/stores/connectionStore.ts";
import { useQueryStore } from "../../apps/desktop/src/stores/queryStore.ts";
import type { ConnectionConfig } from "../../apps/desktop/src/types/database.ts";
import type { QueryResult } from "../../apps/desktop/src/types/database.ts";

function installMemoryStorage() {
  const values = new Map<string, string>();
  const original = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
      removeItem: (key: string) => values.delete(key),
      clear: () => values.clear(),
    },
  });
  return () => {
    if (original) Object.defineProperty(globalThis, "localStorage", original);
    else Reflect.deleteProperty(globalThis, "localStorage");
  };
}

function conn(id: string): ConnectionConfig {
  return {
    id,
    name: id,
    db_type: "postgres",
    host: "localhost",
    port: 5432,
    username: "postgres",
    password: "",
  };
}

test("setErrorResult stops loading and shows the error result", () => {
  setActivePinia(createPinia());
  const store = useQueryStore();
  const tabId = store.createTab("conn-1", "db", "users", "data");

  store.setExecuting(tabId, true);
  store.setErrorResult(tabId, new Error("metadata failed"));

  const tab = store.tabs.find((item) => item.id === tabId);
  assert.equal(tab?.isExecuting, false);
  assert.equal(tab?.isCancelling, false);
  assert.equal(tab?.executionId, undefined);
  assert.deepEqual(tab?.result?.columns, ["Error"]);
  assert.deepEqual(tab?.result?.rows, [["metadata failed"]]);
});

test("evicting cached tab results releases multi-result payloads and sessions", async () => {
  const restoreStorage = installMemoryStorage();
  setActivePinia(createPinia());
  const connectionStore = useConnectionStore();
  const store = useQueryStore();
  const originalFetch = globalThis.fetch;
  let executeCount = 0;
  const closedSessions: string[] = [];

  connectionStore.addEphemeralConnection(conn("conn-1"));

  globalThis.fetch = (async (input, init) => {
    const url = String(input);
    if (url === "/api/query/execute-multi") {
      executeCount++;
      const results: QueryResult[] = [
        {
          columns: ["id"],
          rows: [[executeCount]],
          affected_rows: 0,
          execution_time_ms: 1,
          session_id: `session-${executeCount}`,
        },
        {
          columns: ["detail"],
          rows: [[`payload-${executeCount}`]],
          affected_rows: 0,
          execution_time_ms: 1,
        },
      ];
      return new Response(JSON.stringify(results), { status: 200, headers: { "Content-Type": "application/json" } });
    }
    if (url === "/api/query/close-session") {
      const body = JSON.parse(String(init?.body ?? "{}"));
      closedSessions.push(body.sessionId);
      return new Response(JSON.stringify(true), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    if (url === "/api/query/analyze-editability") {
      return new Response(JSON.stringify({ editable: false, reason: "complex-source" }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    if (url === "/api/query/prepare-pagination-plan") {
      const body = JSON.parse(String(init?.body ?? "{}"));
      return new Response(
        JSON.stringify({
          sqlToExecute: body.options.sql,
          useAgentResultSession: false,
        }),
        {
          status: 200,
          headers: { "Content-Type": "application/json" },
        },
      );
    }
    return new Response("unexpected request", { status: 500 });
  }) as typeof fetch;

  try {
    const tabIds: string[] = [];
    for (let i = 0; i < 7; i++) {
      const tabId = store.createTab("conn-1", "db", `Query ${i + 1}`);
      tabIds.push(tabId);
      await store.executeTabSql(tabId, `select ${i + 1}; select ${i + 1} as detail`);
    }

    const evicted = store.tabs.find((tab) => tab.id === tabIds[0]);
    assert.equal(executeCount, 7);
    assert.equal(evicted?.result, undefined);
    assert.equal(evicted?.results, undefined);
    assert.equal(evicted?.activeResultIndex, undefined);
    assert.equal(evicted?.resultSessionId, undefined);
    assert.equal(evicted?.resultEvicted, true);
    assert.deepEqual(closedSessions, ["session-1"]);
  } finally {
    globalThis.fetch = originalFetch;
    restoreStorage();
  }
});

test("result cache eviction keeps recently accessed inactive tabs", async () => {
  const restoreStorage = installMemoryStorage();
  setActivePinia(createPinia());
  const connectionStore = useConnectionStore();
  const store = useQueryStore();
  const originalFetch = globalThis.fetch;
  let executeCount = 0;

  connectionStore.addEphemeralConnection(conn("conn-1"));

  globalThis.fetch = (async (input) => {
    const url = String(input);
    if (url === "/api/query/execute-multi") {
      executeCount++;
      const results: QueryResult[] = [
        {
          columns: ["id"],
          rows: [[executeCount]],
          affected_rows: 0,
          execution_time_ms: 1,
          session_id: `session-${executeCount}`,
        },
      ];
      return new Response(JSON.stringify(results), { status: 200, headers: { "Content-Type": "application/json" } });
    }
    if (url === "/api/query/close-session") {
      return new Response(JSON.stringify(true), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    if (url === "/api/query/analyze-editability") {
      return new Response(JSON.stringify({ editable: false, reason: "complex-source" }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    if (url === "/api/query/prepare-pagination-plan") {
      return new Response(JSON.stringify({ sqlToExecute: "select 1", useAgentResultSession: false }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    return new Response("unexpected request", { status: 500 });
  }) as typeof fetch;

  try {
    const tabIds: string[] = [];
    for (let i = 0; i < 6; i++) {
      const tabId = store.createTab("conn-1", "db", `Query ${i + 1}`);
      tabIds.push(tabId);
      await store.executeTabSql(tabId, `select ${i + 1}`);
    }

    store.activeTabId = tabIds[0];
    await new Promise((resolve) => setTimeout(resolve, 1));

    const tabId = store.createTab("conn-1", "db", "Query 7");
    tabIds.push(tabId);
    await store.executeTabSql(tabId, "select 7");

    const recentlyViewed = store.tabs.find((tab) => tab.id === tabIds[0]);
    const leastRecentlyUsed = store.tabs.find((tab) => tab.id === tabIds[1]);
    assert.ok(recentlyViewed?.result);
    assert.equal(recentlyViewed?.resultEvicted, undefined);
    assert.equal(leastRecentlyUsed?.result, undefined);
    assert.equal(leastRecentlyUsed?.resultEvicted, true);
  } finally {
    globalThis.fetch = originalFetch;
    restoreStorage();
  }
});

test("closing tabs clears removed result payloads before dropping tab references", async () => {
  const restoreStorage = installMemoryStorage();
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () => {
    return new Response(JSON.stringify(true), { status: 200, headers: { "Content-Type": "application/json" } });
  }) as typeof fetch;
  try {
    setActivePinia(createPinia());
    const store = useQueryStore();
    const keepId = store.createTab("conn-1", "db", "keep");
    const closeId = store.createTab("conn-1", "db", "close");
    const closingTab = store.tabs.find((item) => item.id === closeId);

    assert.ok(closingTab);
    closingTab.result = {
      columns: ["payload"],
      rows: [[new Array(10_000).fill("x").join("")]],
      affected_rows: 0,
      execution_time_ms: 1,
      session_id: "session-close",
    };
    closingTab.results = [closingTab.result];
    closingTab.activeResultIndex = 0;
    closingTab.resultSessionId = "session-close";

    store.closeOtherTabs(keepId);
    await new Promise((resolve) => setTimeout(resolve, 0));

    assert.equal(closingTab.result, undefined);
    assert.equal(closingTab.results, undefined);
    assert.equal(closingTab.activeResultIndex, undefined);
    assert.equal(closingTab.resultSessionId, undefined);
  } finally {
    globalThis.fetch = originalFetch;
    restoreStorage();
  }
});

test("starting a new query clears the previous result payload immediately", async () => {
  const restoreStorage = installMemoryStorage();
  setActivePinia(createPinia());
  const connectionStore = useConnectionStore();
  const store = useQueryStore();
  const originalFetch = globalThis.fetch;

  connectionStore.addEphemeralConnection(conn("conn-1"));
  const tabId = store.createTab("conn-1", "db", "Query");
  const tab = store.tabs.find((item) => item.id === tabId);
  assert.ok(tab);
  tab.result = {
    columns: ["old"],
    rows: [[new Array(10_000).fill("old").join("")]],
    affected_rows: 0,
    execution_time_ms: 1,
  };

  globalThis.fetch = (async (input) => {
    const url = String(input);
    if (url === "/api/query/prepare-pagination-plan") {
      return new Response(JSON.stringify({ sqlToExecute: "select 1", useAgentResultSession: false }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    if (url === "/api/query/execute-multi") {
      return new Response(JSON.stringify([{ columns: ["new"], rows: [[1]], affected_rows: 0, execution_time_ms: 1 }]), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    if (url === "/api/query/analyze-editability") {
      return new Response(JSON.stringify({ editable: false, reason: "complex-source" }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    return new Response("unexpected request", { status: 500 });
  }) as typeof fetch;

  try {
    const execution = store.executeTabSql(tabId, "select 1");
    assert.equal(tab.result, undefined);
    assert.equal(tab.results, undefined);
    await execution;
    assert.deepEqual(tab.result?.columns, ["new"]);
  } finally {
    globalThis.fetch = originalFetch;
    restoreStorage();
  }
});

test("query execution finishes without waiting for metadata analysis", async () => {
  const restoreStorage = installMemoryStorage();
  setActivePinia(createPinia());
  const connectionStore = useConnectionStore();
  const store = useQueryStore();
  const originalFetch = globalThis.fetch;

  connectionStore.addEphemeralConnection(conn("conn-1"));
  const tabId = store.createTab("conn-1", "db", "Query");
  const tab = store.tabs.find((item) => item.id === tabId);
  assert.ok(tab);

  let resolveMetadata: ((value: Response) => void) | undefined;
  globalThis.fetch = (async (input) => {
    const url = String(input);
    if (url === "/api/query/prepare-pagination-plan") {
      return new Response(JSON.stringify({ sqlToExecute: "select id from users", useAgentResultSession: false }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    if (url === "/api/query/execute-multi") {
      return new Response(JSON.stringify([{ columns: ["id"], rows: [[1]], affected_rows: 0, execution_time_ms: 1 }]), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    if (url === "/api/query/analyze-editability") {
      return new Promise<Response>((resolve) => {
        resolveMetadata = resolve;
      });
    }
    return new Response("unexpected request", { status: 500 });
  }) as typeof fetch;

  try {
    await store.executeTabSql(tabId, "select id from users");

    assert.equal(tab.isExecuting, false);
    assert.equal(tab.executionId, undefined);
    assert.deepEqual(tab.result?.columns, ["id"]);

    resolveMetadata?.(
      new Response(JSON.stringify({ editable: false, reason: "complex-source" }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
  } finally {
    globalThis.fetch = originalFetch;
    restoreStorage();
  }
});

test("normal query execution does not create a tab-scoped client session", async () => {
  const restoreStorage = installMemoryStorage();
  setActivePinia(createPinia());
  const connectionStore = useConnectionStore();
  const store = useQueryStore();
  const originalFetch = globalThis.fetch;

  connectionStore.addEphemeralConnection(conn("conn-1"));
  const tabId = store.createTab("conn-1", "db", "Query");
  let executeBody: any;

  globalThis.fetch = (async (input, init) => {
    const url = String(input);
    if (url === "/api/query/prepare-pagination-plan") {
      return new Response(JSON.stringify({ sqlToExecute: "select 1", useAgentResultSession: false }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    if (url === "/api/query/execute-multi") {
      executeBody = JSON.parse(String(init?.body ?? "{}"));
      return new Response(JSON.stringify([{ columns: ["id"], rows: [[1]], affected_rows: 0, execution_time_ms: 1 }]), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    if (url === "/api/query/analyze-editability") {
      return new Response(JSON.stringify({ editable: false, reason: "complex-source" }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    return new Response("unexpected request", { status: 500 });
  }) as typeof fetch;

  try {
    await store.executeTabSql(tabId, "select 1");

    assert.equal(executeBody.clientSessionId, undefined);
    assert.equal(executeBody.timeoutSecs, 30);
  } finally {
    globalThis.fetch = originalFetch;
    restoreStorage();
  }
});

test("tab reuse is scoped by mode and schema instead of title alone", () => {
  const restoreStorage = installMemoryStorage();
  try {
    setActivePinia(createPinia());
    const store = useQueryStore();

    const dataTabId = store.createTab("conn-1", "db", "users", "data", "public");
    const sourceTabId = store.createTab("conn-1", "db", "users", "query", "public");
    const otherSchemaTabId = store.createTab("conn-1", "db", "users", "data", "audit");
    const reusedDataTabId = store.createTab("conn-1", "db", "users", "data", "public");

    assert.notEqual(sourceTabId, dataTabId);
    assert.notEqual(otherSchemaTabId, dataTabId);
    assert.equal(reusedDataTabId, dataTabId);
    assert.equal(store.tabs.length, 3);
  } finally {
    restoreStorage();
  }
});
