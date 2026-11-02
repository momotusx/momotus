import { expect, test } from "bun:test";

// QA r2, Q7 row 20: a production bundle runs history.ts before the router stamps the first entry's depth. The base
// must still be that entry's depth, the page before the app (one here) included, or the app's Back leaves the app.
test("a first entry the router has not stamped yet is the base, at the router's depth", async () => {
  const g = globalThis as Record<string, unknown>;
  const names = ["window", "history", "History", "sessionStorage", "performance"] as const;
  const before = Object.fromEntries(names.map((n) => [n, g[n]]));
  class FakeHistory {
    length = 2;
    raw: unknown = null;
    get state() {
      return this.raw;
    }
    pushState(s: unknown) {
      this.raw = s;
      this.length++;
    }
    replaceState(s: unknown) {
      this.raw = s;
    }
    back() {}
    forward() {}
    go() {}
  }
  const stored = new Map<string, string>();
  Object.assign(g, {
    window: { addEventListener() {} },
    History: FakeHistory,
    history: new FakeHistory(),
    sessionStorage: { getItem: (k: string) => stored.get(k) ?? null, setItem: (k: string, v: string) => void stored.set(k, v) },
    performance: { now: () => 0, getEntriesByType: () => [{ type: "navigate" }] },
  });
  try {
    const { historyBase } = await import("./history");
    expect(historyBase()).toBe(1);
    expect(stored.get("mq.historyBase")).toBe("1");
  } finally {
    Object.assign(g, before);
  }
});
