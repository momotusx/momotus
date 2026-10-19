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
