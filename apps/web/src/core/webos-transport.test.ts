import { afterEach, expect, test } from "bun:test";
import { relaunch, takeToken, TOKEN_REFUSED } from "../lib/webos";
import { webosTransport } from "./webos-transport";

// A Storage over a Map, and the page's location, history, Luna and WebSocket, faked on globalThis.
const storage = () => {
  const m = new Map<string, string>();
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) } as Storage;
};
const saved = ["location", "history", "sessionStorage", "WebSocket", "PalmSystem", "PalmServiceBridge", "window"].map((k) => [k, Object.getOwnPropertyDescriptor(globalThis, k)] as const);
afterEach(() => {
  for (const [k, d] of saved) d ? Object.defineProperty(globalThis, k, d) : delete (globalThis as Record<string, unknown>)[k];
});
const page = (hash = "") => {
  const seen = { reloads: 0, replaced: [] as string[], url: "" };
  const location = { hash, pathname: "/", search: "", reload: () => seen.reloads++, replace: (u: string) => seen.replaced.push(u) };
  const history = { state: { at: 1 }, replaceState: (_s: unknown, _t: string, u: string) => ((seen.url = u), (location.hash = "")) };
  Object.assign(globalThis, { location, history, sessionStorage: storage(), window: globalThis });
  return seen;
};
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
  close(code: number) {
    this.dispatchEvent(Object.assign(new Event("close"), { code }));
  }
}

test("the token leaves the fragment for the tab's storage and survives a reload", () => {
  const seen = page("#t=abc&stub=file%3A%2F%2F%2Fmedia%2Fapp%2Findex.html");
  const store = storage();
  expect(takeToken(store)).toBe("abc");
  expect(seen.url).toBe("/");
  expect(store.getItem("mqStub")).toBe("file:///media/app/index.html");
  page("");
  expect(takeToken(store)).toBe("abc");
});

test("the token goes first, sends wait for the open socket, and a close reloads", () => {
  const seen = page();
  Object.assign(globalThis, { WebSocket: FakeSocket });
  const t = webosTransport("ws://127.0.0.1:47801/ws", "tok");
  t.send("a");
  FakeSocket.last.open();
  t.send("b");
  expect(FakeSocket.last.sent).toEqual(["tok", "a", "b"]);
  FakeSocket.last.close(1006);
  expect(seen.reloads).toBe(1);
});

test("a refused token, or a socket that never opened, starts the service again for a new token", () => {
  const seen = page();
  const store = storage();
  const calls: string[] = [];
  class Bridge {
    onservicecallback: ((m: string) => void) | null = null;
    call(uri: string) {
      calls.push(uri);
      this.onservicecallback?.(JSON.stringify({ returnValue: true, token: "new" }));
    }
  }
  Object.assign(globalThis, { WebSocket: FakeSocket, PalmSystem: { identifier: "app.momotus.webos" }, PalmServiceBridge: Bridge });
  webosTransport("ws://x", "old");
  FakeSocket.last.open();
  FakeSocket.last.close(TOKEN_REFUSED);
  expect(calls).toEqual(["luna://app.momotus.webos.service/start"]);
  expect(seen.reloads).toBe(1);
  // Three relaunches a minute at most, so a service that never comes up does not spin.
  relaunch(store);
  relaunch(store);
  relaunch(store);
  relaunch(store);
  expect(calls.length).toBe(4);
  expect(store.getItem("mqToken")).toBe("new");
});

test("without Luna a relaunch goes back to the packaged stub", () => {
  const seen = page();
  const store = storage();
  store.setItem("mqStub", "file:///media/app/index.html");
  relaunch(store);
  expect(seen.replaced).toEqual(["file:///media/app/index.html"]);
});

test("reloads after a close are capped too, then the service is started again", () => {
  const seen = page();
  const calls: string[] = [];
  class Bridge {
    onservicecallback: ((m: string) => void) | null = null;
    call(uri: string) {
      calls.push(uri);
    }
  }
  // `identifier` is "<app id> <process id>" on a TV.
  Object.assign(globalThis, { WebSocket: FakeSocket, PalmSystem: { identifier: "app.momotus.webos 1234" }, PalmServiceBridge: Bridge });
  for (let i = 0; i < 4; i++) {
    webosTransport("ws://x", "tok");
    FakeSocket.last.open();
    FakeSocket.last.close(1013);
  }
  expect(seen.reloads).toBe(3);
  expect(calls).toEqual(["luna://app.momotus.webos.service/start"]);
});

test("a relaunch that cannot go anywhere says the app could not start", async () => {
  // Its own instance (the query): earlier tests here already spent the relaunch cap of the shared one.
  const fresh = "../lib/webos?fresh";
  const { startFailed, couldNotStart, relaunch } = (await import(fresh)) as typeof import("../lib/webos");
  page();
  const store = storage();
  expect(startFailed()).toBe(false);
  // No Luna and no stub address.
  relaunch(store);
  expect(startFailed()).toBe(true);
  expect(couldNotStart).toBe("Momotus could not start. Close the app and open it again.");
});
