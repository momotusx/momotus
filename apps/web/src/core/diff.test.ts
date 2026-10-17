import { describe, expect, test } from "bun:test";
import { applyDiffs } from "./diff";
import { Bridge, type Transport } from "./bridge";

describe("applyDiffs", () => {
  test("matches the core semantics for every op", () => {
    const out = applyDiffs<number>(
      [],
      [
        { op: "pushBack", value: 1 },
        { op: "pushBack", value: 2 },
        { op: "insert", index: 1, value: 9 },
        { op: "set", index: 0, value: 5 },
        { op: "remove", index: 2 },
        { op: "pushFront", value: 7 },
        { op: "append", values: [8, 8] },
        { op: "popBack" },
        { op: "popFront" },
        { op: "truncate", length: 2 },
      ],
    );
    expect(out).toEqual([5, 9]);
    expect(applyDiffs(out, [{ op: "reset", values: [1] }, { op: "clear" }])).toEqual([]);
  });

  test("does not mutate its input", () => {
    const input = [1, 2, 3];
    applyDiffs(input, [{ op: "set", index: 0, value: 9 }]);
    expect(input).toEqual([1, 2, 3]);
  });
});

function loopback(): { transport: Transport; sent: unknown[]; deliver: (m: unknown) => void } {
  const sent: unknown[] = [];
  let handler: (text: string) => void = () => {};
  return {
    sent,
    deliver: (m) => handler(JSON.stringify(m)),
    transport: {
      send: (text) => sent.push(JSON.parse(text)),
      onMessage: (h) => (handler = h),
      onClose: () => {},
    },
  };
}

describe("Bridge", () => {
  test("pairs replies with requests and routes pushes by subscription", async () => {
    const { transport, sent, deliver } = loopback();
    const bridge = new Bridge(transport);
    const batches: unknown[][] = [];
    const opening = bridge.subscribe("home", {}, (u) => batches.push(u));
    expect(sent[0]).toEqual({ id: 1, op: "subscribe", view: "home", params: {} });
    deliver({ id: 1, ok: true, result: { sub: 4, snapshot: { rows: [] } } });
    const view = await opening;
    expect(view.snapshot).toEqual({ rows: [] });
    deliver({ sub: 4, updates: [{ kind: "rows", diffs: [] }] });
    deliver({ sub: 5, updates: [{ kind: "ignored" }] });
    expect(batches).toEqual([[{ kind: "rows", diffs: [] }]]);
  });

  test("rejects failed requests with the core error", async () => {
    const { transport, deliver } = loopback();
    const bridge = new Bridge(transport);
    const pending = bridge.request("subscribe", { view: "nope" });
    deliver({ id: 1, ok: false, error: "unknown view: nope" });
    await expect(pending).rejects.toThrow("unknown view: nope");
  });
});
