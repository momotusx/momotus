import { afterEach, expect, test } from "bun:test";
import words from "../../../shared/update-words.json";
import { listenUpdates, sendUpdate, webosTransport } from "./webos-transport";
import { takeUpdate, updateRow, updateState } from "./webos-update";

const saved = ["location", "history", "sessionStorage", "WebSocket", "window"].map((k) => [k, Object.getOwnPropertyDescriptor(globalThis, k)] as const);
afterEach(() => {
  for (const [k, d] of saved) d ? Object.defineProperty(globalThis, k, d) : delete (globalThis as Record<string, unknown>)[k];
});

class FakeSocket extends EventTarget {
  static OPEN = 1;
  static last: FakeSocket;
  readyState = 0;
  sent: string[] = [];
  constructor(public url: string) {
    super();
    FakeSocket.last = this;
  }
  send(text: string) {
    this.sent.push(text);
  }
  open() {
    this.readyState = 1;
    this.dispatchEvent(new Event("open"));
  }
  say(text: string) {
    this.dispatchEvent(Object.assign(new Event("message"), { data: text }));
  }
}

test("update messages go to the update listener, never to the core's handler", () => {
  Object.assign(globalThis, { location: { reload() {}, replace() {} }, window: globalThis, WebSocket: FakeSocket });
  const t = webosTransport("ws://127.0.0.1:47801/ws", "tok");
  const core: string[] = [];
  const updates: string[] = [];
  t.onMessage((m) => core.push(m));
  listenUpdates((m) => updates.push(m));
  FakeSocket.last.open();
  expect(sendUpdate({ op: "check", asked: true })).toBe(true);
  const asked = FakeSocket.last.sent[1] ?? "";
  expect(JSON.parse(asked)).toMatchObject({ host: "update", op: "check", asked: true });
  expect(asked.startsWith('{"host":"update"')).toBe(true);
  FakeSocket.last.say('{"host":"update","id":null,"state":{"kind":"checking"}}');
  FakeSocket.last.say('{"kind":"core"}');
  expect(updates).toEqual(['{"host":"update","id":null,"state":{"kind":"checking"}}']);
  expect(core).toEqual(['{"kind":"core"}']);
});

test("every answer and push replaces the state; null means no updates here", () => {
  takeUpdate('{"host":"update","id":3,"state":{"kind":"ready","version":"0.5.0"}}');
  expect(updateState()).toEqual({ kind: "ready", version: "0.5.0" });
  takeUpdate('{"host":"update","id":null,"state":null}');
  expect(updateState()).toBeNull();
  takeUpdate("not json");
  expect(updateState()).toBeNull();
});

test("the row offers one action at most, none while it works, in the shared words", () => {
  expect(updateRow({ kind: "idle" })).toMatchObject({ line: "", action: { label: words.check } });
  expect(updateRow({ kind: "current" })).toMatchObject({ line: words.current, action: { label: words.check } });
  expect(updateRow({ kind: "failed" })).toMatchObject({ line: words.failed, action: { label: words.check } });
  expect(updateRow({ kind: "checking" })).toEqual({ line: words.checking, action: null });
  expect(updateRow({ kind: "downloading", version: "0.5.0", percent: 40 })).toEqual({ line: "Downloading 0.5.0, 40%", action: null });
  expect(updateRow({ kind: "ready", version: "0.5.0" })).toMatchObject({
    line: "Version 0.5.0 is ready. Momotus closes to install it.",
    action: { label: words.install, install: true },
  });
  expect(updateRow({ kind: "installing", version: "0.5.0" })).toEqual({ line: "Installing 0.5.0. Momotus opens again when it is done.", action: null });
});
