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
  /** Records what one view saw, in order: its snapshot, then each batch. */
  const recorder = () => {
    const seen: unknown[] = [];
    return { seen, on: { snapshot: (s: unknown) => seen.push({ snapshot: s }), updates: (u: unknown[]) => seen.push(u) } };
  };

  test("pairs replies with requests and routes pushes by subscription", async () => {
    const { transport, sent, deliver } = loopback();
    const bridge = new Bridge(transport);
    const home = recorder();
    const opening = bridge.subscribe("home", {}, home.on);
    expect(sent[0]).toEqual({ id: 1, op: "subscribe", view: "home", params: {} });
    deliver({ id: 1, ok: true, result: { sub: 4, snapshot: { rows: [] } } });
    expect((await opening).sub).toBe(4);
    deliver({ sub: 4, updates: [{ kind: "rows", diffs: [] }] });
    deliver({ sub: 3, updates: [{ kind: "closed view" }] });
    expect(home.seen).toEqual([{ snapshot: { rows: [] } }, [{ kind: "rows", diffs: [] }]]);
  });

  test("a push in the same batch as its reply, or before it, is applied after the snapshot", async () => {
    const { transport, deliver } = loopback();
    const bridge = new Bridge(transport);
    const a = recorder();
    const b = recorder();
    const openA = bridge.subscribe("home", {}, a.on);
    const openB = bridge.subscribe("title", {}, b.on);
    // One synchronous batch, as a shell hands it over: B pushes before its
    // reply, A's reply and push sit together, then B's reply and push.
    deliver({ sub: 2, updates: ["b1"] });
    deliver({ id: 1, ok: true, result: { sub: 1, snapshot: "A" } });
    deliver({ sub: 1, updates: ["a1"] });
    deliver({ sub: 2, updates: ["b2"] });
    deliver({ id: 2, ok: true, result: { sub: 2, snapshot: "B" } });
    deliver({ sub: 2, updates: ["b3"] });
    await Promise.all([openA, openB]);
    expect(a.seen).toEqual([{ snapshot: "A" }, ["a1"]]);
    expect(b.seen).toEqual([{ snapshot: "B" }, ["b1"], ["b2"], ["b3"]]);
  });

  test("pushes for a closed view are dropped, not kept", async () => {
    const { transport, deliver } = loopback();
    const bridge = new Bridge(transport);
    const a = recorder();
    const opening = bridge.subscribe("home", {}, a.on);
    deliver({ id: 1, ok: true, result: { sub: 1, snapshot: "A" } });
    const view = await opening;
    const closing = view.close();
    deliver({ sub: 1, updates: ["late"] });
    deliver({ id: 2, ok: true, result: {} });
    await closing;
    expect(a.seen).toEqual([{ snapshot: "A" }]);
    expect((bridge as unknown as { early: Map<number, unknown> }).early.size).toBe(0);
  });

  test("rejects failed requests with the core error", async () => {
    const { transport, deliver } = loopback();
    const bridge = new Bridge(transport);
    const pending = bridge.request("subscribe", { view: "nope" });
    deliver({ id: 1, ok: false, error: "unknown view: nope" });
    await expect(pending).rejects.toThrow("unknown view: nope");
  });
});
