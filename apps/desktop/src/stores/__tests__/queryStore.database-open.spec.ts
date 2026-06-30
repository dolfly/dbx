import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

function installLocalStorage() {
  const data = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: vi.fn((key: string) => data.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => data.set(key, value)),
    removeItem: vi.fn((key: string) => data.delete(key)),
  });
}

describe("queryStore database open state", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.unstubAllGlobals();
    installLocalStorage();
    setActivePinia(createPinia());
  });

  it("tracks whether a connection database has open tabs", async () => {
    const { useQueryStore } = await import("@/stores/queryStore");
    const store = useQueryStore();

    const tabId = store.createTab("pg-1", "app", "query_1");

    expect(store.isDatabaseOpen("pg-1", "app")).toBe(true);
    expect(store.isDatabaseOpen("pg-1", "analytics")).toBe(false);
    expect(store.isDatabaseOpen("pg-2", "app")).toBe(false);

    store.updateDatabase(tabId, "analytics");

    expect(store.isDatabaseOpen("pg-1", "app")).toBe(false);
    expect(store.isDatabaseOpen("pg-1", "analytics")).toBe(true);

    store.closeTab(tabId);

    expect(store.isDatabaseOpen("pg-1", "analytics")).toBe(false);
  });
});
