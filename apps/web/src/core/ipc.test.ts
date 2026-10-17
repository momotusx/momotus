import { describe, expect, test } from "bun:test";
import { Bridge, BridgeError, ipcTransport, type ShellHost } from "./bridge";

/** A fake shell: records what the page posts and calls back like a shell. */
function shell() {
  const posted: Record<string, unknown>[] = [];
  const host: ShellHost = {};
  const transport = ipcTransport((text) => posted.push(JSON.parse(text)), undefined, host);
  // The shell answers with the nonce this document's hello carried.
  const nonce = () => String(posted[0]?.nonce);
  return {
    posted,
    transport,
    nonce,
    ready: (token: string, generation = 1, n = nonce()) => host.__mq?.ready({ nonce: n, generation, token, platform: "linux" }),
    recv: (...messages: unknown[]) => host.__mq?.recv(nonce(), messages.map((m) => JSON.stringify(m))),
    /** A batch the shell bound to another document's nonce. */
    recvFor: (n: string, ...messages: unknown[]) => host.__mq?.recv(n, messages.map((m) => JSON.stringify(m))),
  };
}

describe("ipcTransport", () => {
  test("only hello goes out before ready; requests then follow in order with the token", async () => {
    const s = shell();
    const bridge = new Bridge(s.transport);
    const first = bridge.request("ping");
    const second = bridge.request("homeCatalogs");
    expect(s.posted).toEqual([{ op: "hello", nonce: s.nonce() }]);
    s.ready("t1");
    expect(s.posted.slice(1)).toEqual([
      { token: "t1", id: 1, op: "ping" },
      { token: "t1", id: 2, op: "homeCatalogs" },
    ]);
    s.recv({ id: 2, ok: true, result: [] }, { id: 1, ok: true, result: {} });
    expect(await first).toEqual({});
    expect(await second).toEqual([]);
    void bridge.request("ping");
    expect(s.posted.at(-1)).toEqual({ token: "t1", id: 3, op: "ping" });
  });

  test("a ready answering another document's hello is refused", () => {
    const s = shell();
    const bridge = new Bridge(s.transport);
    void bridge.request("ping");
    // A ready the shell sent the previous document, executed in this one.
    expect(s.ready("stale", 1, "nonce-of-the-previous-document")).toBe(false);
    expect(s.posted).toEqual([{ op: "hello", nonce: s.nonce() }]);
    expect(s.ready("t2", 2)).toBe(true);
    expect(s.posted.at(-1)).toEqual({ token: "t2", id: 1, op: "ping" });
  });

  test("a second ready for the same document changes nothing", () => {
    const s = shell();
    const bridge = new Bridge(s.transport);
    expect(s.ready("t1")).toBe(true);
    expect(s.ready("t2")).toBe(false);
    void bridge.request("ping");
    expect(s.posted.at(-1)).toEqual({ token: "t1", id: 1, op: "ping" });
  });

  test("a batch made for the previous document is dropped whole, before and after this one's ready", async () => {
    // The shell submitted it for the old document; a navigation committed
    // before it ran. Its ids collide with this document's: both start at 1.
    const old = shell();
    const s = shell();
    const bridge = new Bridge(s.transport);
    const seen: unknown[] = [];
    let opened = false;
    const opening = bridge.subscribe("home", {}, { snapshot: (x) => seen.push({ snapshot: x }), updates: (u) => seen.push(u) });
    void opening.then(() => (opened = true));
    const stale = [{ sub: 1, updates: ["old push"] }, { id: 1, ok: true, result: { sub: 1, snapshot: "old" } }, { sub: 1, updates: ["old after"] }];
    s.recvFor(old.nonce(), ...stale);
    s.ready("t2");
    s.recvFor(old.nonce(), ...stale);
    await Promise.resolve();
    expect(opened).toBe(false);
    expect(seen).toEqual([]);
    s.recv({ sub: 1, updates: ["b1"] }, { id: 1, ok: true, result: { sub: 1, snapshot: "new" } });
    await opening;
    // A push of the old document for the same id after the view is open.
    s.recvFor(old.nonce(), { sub: 1, updates: ["old push"] });
    s.recv({ sub: 1, updates: ["b2"] });
    expect(seen).toEqual([{ snapshot: "new" }, ["b1"], ["b2"]]);
  });

  test("a batch keeps its order, pushes included", async () => {
    const s = shell();
    const bridge = new Bridge(s.transport);
    s.ready("t1");
    const seen: unknown[] = [];
    const opening = bridge.subscribe("home", {}, { snapshot: (x) => seen.push({ snapshot: x }), updates: (u) => seen.push(u) });
    s.recv({ id: 1, ok: true, result: { sub: 3, snapshot: "S" } }, { sub: 3, updates: ["u1"] }, { sub: 3, updates: ["u2"] });
    await opening;
    expect(seen).toEqual([{ snapshot: "S" }, ["u1"], ["u2"]]);
  });

  test("a shell that never answers: past the waiting limit the link closes", async () => {
    const s = shell();
    const bridge = new Bridge(s.transport);
    const waiting = Array.from({ length: 256 }, () => bridge.request("ping").catch((e: unknown) => e));
    const over = bridge.request("ping").catch((e: unknown) => e);
    const errors = await Promise.all([...waiting, over]);
    expect(errors.every((e) => e instanceof BridgeError && e.message === "connection closed")).toBe(true);
    // A late ready does not reopen a closed link.
    s.ready("t1");
    expect(s.posted).toEqual([{ op: "hello", nonce: s.nonce() }]);
  });

  test("host events on a reserved subscription reach their listener", () => {
    const s = shell();
    const bridge = new Bridge(s.transport);
    s.ready("t1");
    const events: unknown[] = [];
    const stop = bridge.listen(0, (u) => events.push(...u));
    s.recv({ sub: 0, updates: [{ playId: 1, kind: "time", sec: 3 }] });
    stop();
    s.recv({ sub: 0, updates: [{ playId: 1, kind: "time", sec: 4 }] });
    expect(events).toEqual([{ playId: 1, kind: "time", sec: 3 }]);
  });
});
