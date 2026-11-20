/** The webOS socket to the app's own service (plans/webos.md section 1): the token from `takeToken` goes first, sends
 * queue until the socket opens. Any close after it opened reloads the page (a restarted core comes back through the
 * normal start path); a refused token, or a socket that never opened (the service is gone), starts the service
 * again through `relaunch`. The service's keystore and its updates (plans/auto-update.md) answer on the same socket and
 * never reach the Bridge. */

import type { Transport } from "./bridge";
import { relaunch, reloadAllowed, takeToken, TOKEN_REFUSED } from "../lib/webos";

const KEYSTORE = '{"host":"keystore"';
const UPDATE = '{"host":"update"';
const own = (text: string) => text.startsWith(KEYSTORE) || text.startsWith(UPDATE);

type Reply = { id: number; ok: boolean; value?: Record<string, unknown>; error?: string };
let send: ((text: string) => void) | null = null;
let onUpdate: ((text: string) => void) | null = null;

/** Who hears the service's update messages (webos-update.ts): every answer and every pushed state. */
export const listenUpdates = (listen: (text: string) => void) => void (onUpdate = listen);

/** One update request (`state`, `check`, `install`); false with no service socket. */
export function sendUpdate(op: { op: "state" | "check" | "install"; asked?: boolean }): boolean {
  if (!send) return false;
  send(JSON.stringify({ host: "update", id: ++next, ...op }));
  return true;
}
let next = 0;
const waiting = new Map<number, { ok: (value: Record<string, unknown>) => void; fail: (e: Error) => void }>();

/** One keystore call: `read` answers the whole store; `write` sets one key (null deletes) and answers once it is on disk. */
export function keystore(op: { op: "read" } | { op: "write"; key: string; value: unknown }): Promise<Record<string, unknown>> {
  if (!send) return Promise.reject(new Error("no webOS service"));
  const id = ++next;
  const say = send;
  return new Promise((ok, fail) => {
    waiting.set(id, { ok, fail });
    // `host` first: the service tells its own messages by that prefix.
    say(JSON.stringify({ host: "keystore", id, ...op }));
  });
}

function answer(text: string) {
  let reply: Reply;
  try {
    reply = JSON.parse(text) as Reply;
  } catch {
    return;
  }
  const call = waiting.get(reply.id);
  if (!call) return;
  waiting.delete(reply.id);
  if (reply.ok) call.ok(reply.value ?? {});
  else call.fail(new Error(reply.error ?? "the keystore refused"));
}

export function webosTransport(url: string, token = takeToken()): Transport {
  if (token === null) {
    relaunch();
    return { send() {}, onMessage() {}, onClose() {} };
  }
  const socket = new WebSocket(url);
  const queue: string[] = [];
  let opened = false;
  socket.addEventListener("open", () => {
    opened = true;
    socket.send(token);
    for (const text of queue.splice(0)) socket.send(text);
  });
  socket.addEventListener("message", (event) => {
    const text = String(event.data);
    if (text.startsWith(KEYSTORE)) answer(text);
    else if (text.startsWith(UPDATE)) onUpdate?.(text);
  });
  socket.addEventListener("close", (event) => {
    send = null;
    for (const call of waiting.values()) call.fail(new Error("the webOS service closed"));
    waiting.clear();
    if (opened && event.code !== TOKEN_REFUSED && reloadAllowed()) location.reload();
    else relaunch();
  });
  const transport: Transport = {
    send(text) {
      if (opened && socket.readyState === WebSocket.OPEN) socket.send(text);
      else queue.push(text);
    },
    onMessage(handler) {
      socket.addEventListener("message", (event) => {
        const text = String(event.data);
        if (!own(text)) handler(text);
      });
    },
    onClose(handler) {
      socket.addEventListener("close", handler);
    },
  };
  send = transport.send;
  return transport;
}
