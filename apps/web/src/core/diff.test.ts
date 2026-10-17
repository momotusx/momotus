import { describe, expect, test } from "bun:test";
import { applyDiffs } from "./diff";
import { Bridge, BridgeError, type Transport } from "./bridge";

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

  // A desktop shell's host answers pairing with ids from 2^52 (plans/m3-desktop.md
  // 4.4.5) and in no fixed order against the page's own views.
  const HOST = 2 ** 52;

  test("a view opened after a host subscription still gets its early pushes", async () => {
    const { transport, deliver } = loopback();
    const bridge = new Bridge(transport);
    const pairing = recorder();
    const home = recorder();
    const openP = bridge.subscribe("pairing", {}, pairing.on);
    deliver({ id: 1, ok: true, result: { sub: HOST, snapshot: "P" } });
    await openP;
    const openH = bridge.subscribe("home", {}, home.on);
    deliver({ sub: 1, updates: ["h1"] });
    deliver({ id: 2, ok: true, result: { sub: 1, snapshot: "H" } });
    await openH;
    expect(home.seen).toEqual([{ snapshot: "H" }, ["h1"]]);
  });

  for (const hostFirst of [true, false]) {
    test(`host and page views opening together, ${hostFirst ? "host" : "page"} reply first`, async () => {
      const { transport, deliver } = loopback();
      const bridge = new Bridge(transport);
      const pairing = recorder();
      const home = recorder();
      const openP = bridge.subscribe("pairing", {}, pairing.on);
      const openH = bridge.subscribe("home", {}, home.on);
      deliver({ sub: 1, updates: ["h1"] });
      deliver({ sub: HOST, updates: ["p1"] });
      const replyP = { id: 1, ok: true, result: { sub: HOST, snapshot: "P" } };
      const replyH = { id: 2, ok: true, result: { sub: 1, snapshot: "H" } };
      deliver(hostFirst ? replyP : replyH);
      deliver({ sub: 1, updates: ["h2"] });
      deliver({ sub: HOST, updates: ["p2"] });
      deliver(hostFirst ? replyH : replyP);
      await Promise.all([openP, openH]);
      expect(home.seen).toEqual([{ snapshot: "H" }, ["h1"], ["h2"]]);
      expect(pairing.seen).toEqual([{ snapshot: "P" }, ["p1"], ["p2"]]);
      expect((bridge as unknown as { early: Map<number, unknown> }).early.size).toBe(0);
    });
  }

  test("pushes left over when the last open settles are dropped, failed opens included", async () => {
    const { transport, deliver } = loopback();
    const bridge = new Bridge(transport);
    const opening = bridge.subscribe("home", {}, recorder().on).catch(() => undefined);
    deliver({ sub: 7, updates: ["for a closed view"] });
    deliver({ id: 1, ok: false, error: "unknown view" });
    await opening;
    expect((bridge as unknown as { early: Map<number, unknown> }).early.size).toBe(0);
  });

  test("rejects failed requests with the core error", async () => {
    const { transport, deliver } = loopback();
    const bridge = new Bridge(transport);
    const pending = bridge.request("subscribe", { view: "nope" });
    deliver({ id: 1, ok: false, error: "unknown view: nope" });
    await expect(pending).rejects.toThrow("unknown view: nope");
  });

  test("a structured error code reaches the caller unchanged", async () => {
    const { transport, deliver } = loopback();
    const bridge = new Bridge(transport);
    const coded = bridge.request("syncNow");
    const plain = bridge.request("ping");
    deliver({ id: 1, ok: false, error: "This device has no sync record yet", code: "noSyncRecord" });
    deliver({ id: 2, ok: false, error: "bad request" });
    const a = await coded.catch((e: unknown) => e);
    expect(a).toBeInstanceOf(BridgeError);
    expect((a as BridgeError).code).toBe("noSyncRecord");
    expect((a as BridgeError).message).toBe("This device has no sync record yet");
    const b = await plain.catch((e: unknown) => e);
    expect((b as BridgeError).code).toBeUndefined();
  });
});
