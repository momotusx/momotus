import { expect, test } from "bun:test";
import { Bridge, type Transport } from "./bridge";

/** A transport the test speaks for: `deliver` hands the page a message from the core. */
function fakeTransport() {
  let deliver: (text: string) => void = () => undefined;
  const sent: string[] = [];
  const transport: Transport = {
    send: (text) => void sent.push(text),
    onMessage: (fn) => void (deliver = fn),
    onClose: () => undefined,
  };
  return { transport, sent, deliver: (m: object) => deliver(JSON.stringify(m)) };
}

test("a profile the core says is gone reaches its listeners, and replies still find their requests", async () => {
  const t = fakeTransport();
  const bridge = new Bridge(t.transport);
  const heard: string[] = [];
  const stop = bridge.onProfileGone((p) => heard.push(p));
  const asked = bridge.request<{ x: number }>("ping");
  t.deliver({ profile: "gone", id: "p9" });
  t.deliver({ id: 1, ok: true, result: { x: 1 } });
  expect(await asked).toEqual({ x: 1 });
  expect(heard).toEqual(["p9"]);
  stop();
  t.deliver({ profile: "gone", id: "p8" });
  expect(heard).toEqual(["p9"]);
});

test("the core closing a kids session's views, and making its profile standard, reach their listeners (plans/m11-kids.md 8)", async () => {
  const t = fakeTransport();
  const bridge = new Bridge(t.transport);
  const closed: number[][] = [];
  let access = 0;
  const stopClosed = bridge.onClosed((subs) => closed.push(subs));
  const stopAccess = bridge.onAccess(() => access++);
  const asked = bridge.request<{ x: number }>("ping");
  t.deliver({ closed: "notAllowed", subs: [4, 7] });
  t.deliver({ profile: "access", id: "k" });
  t.deliver({ id: 1, ok: true, result: { x: 1 } });
  expect(await asked).toEqual({ x: 1 });
  expect(closed).toEqual([[4, 7]]);
  expect(access).toBe(1);
  stopClosed();
  stopAccess();
  t.deliver({ closed: "notAllowed", subs: [9] });
  t.deliver({ profile: "access", id: "k" });
  expect(closed).toEqual([[4, 7]]);
  expect(access).toBe(1);
});
