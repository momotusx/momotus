import { createSignal } from "solid-js";

/** LG webOS detection. The user agent carries `Web0S` and a Chrome major (webOS 6 is Chrome 79, webOS 22 is 87);
 * capabilities follow the major, never an exact user-agent table. */

/** The Chrome major in `ua`, or 0 when it names none. */
export const chromeMajor = (ua: string): number => Number(/Chrome\/(\d+)/.exec(ua)?.[1] ?? 0);

export const isWebOSAgent = (ua: string): boolean => /Web0S/.test(ua);

export const isWebOS = (): boolean => typeof navigator !== "undefined" && isWebOSAgent(navigator.userAgent);

/** Calls the platform's own Back: on Home it leaves the app (the system decides where to). */
export function platformBack() {
  (window as unknown as { PalmSystem?: { platformBack?: () => void } }).PalmSystem?.platformBack?.();
}

/** The LG remote's media keys by `keyCode`; Chromium on webOS does not name them in `key`. */
const MEDIA_CODES: Record<number, string> = { 415: "MediaPlay", 19: "MediaPause", 413: "MediaStop", 417: "MediaFastForward", 412: "MediaRewind" };

/** `e.key`, with the LG remote's media keys named on webOS (elsewhere keyCode 19 is the keyboard's Pause). */
export const mediaKey = (e: KeyboardEvent, webos = isWebOS()): string => (webos && MEDIA_CODES[e.keyCode]) || e.key;

type LunaBridge = { onservicecallback: ((message: string) => void) | null; call(uri: string, payload: string): void };
type LunaClass = new () => LunaBridge;
// A bridge the page drops is collected before it answers, so each one is held until its first answer.
const calling = new Set<LunaBridge>();

/** One Luna call; `answer` gets the parsed reply once, or `null` when there is no bridge or the reply is not JSON. */
export function luna(
  uri: string,
  payload: object,
  answer: (reply: Record<string, unknown> | null) => void,
  bridgeClass = (globalThis as { PalmServiceBridge?: LunaClass }).PalmServiceBridge,
) {
  if (!bridgeClass) return answer(null);
  try {
    const bridge = new bridgeClass();
    calling.add(bridge);
    bridge.onservicecallback = (message) => {
      calling.delete(bridge);
      let reply: Record<string, unknown> | null = null;
      try {
        reply = JSON.parse(message) as Record<string, unknown>;
      } catch {
        // Not JSON: no answer.
      }
      answer(reply);
    };
    bridge.call(uri, JSON.stringify(payload));
  } catch {
    answer(null);
  }
}

// The service's socket token (plans/webos.md section 1): the stub puts it in the fragment, the page keeps it per
// tab in sessionStorage, so a reload keeps it and another app's window on the same origin cannot read it.
const TOKEN = "mqToken";
const STUB = "mqStub";
const RELAUNCHES = "mqRelaunches";
const RELOADS = "mqReloads";
/** The close code the service sends when it refuses the token. */
export const TOKEN_REFUSED = 4401;

/** Moves the token (and the stub's address) out of the fragment, before the router stamps its first entry; returns the token. */
export function takeToken(store: Storage = sessionStorage): string | null {
  const fragment = new URLSearchParams(location.hash.slice(1));
  const token = fragment.get("t");
  if (token) {
    store.setItem(TOKEN, token);
    const stub = fragment.get("stub");
    if (stub) store.setItem(STUB, stub);
    history.replaceState(history.state, "", location.pathname + location.search);
  }
  return store.getItem(TOKEN);
}

/** Counts one more `key` event in the last minute; false once 3 already happened there. */
function another(key: string, store: Storage): boolean {
  const now = Date.now();
  const recent = (JSON.parse(store.getItem(key) ?? "[]") as number[]).filter((t) => now - t < 60_000);
  if (recent.length >= 3) return false;
  store.setItem(key, JSON.stringify([...recent, now]));
  return true;
}

/** A reload after the socket closed, at most 3 a minute: a service that keeps closing gets a relaunch instead. */
export const reloadAllowed = (store: Storage = sessionStorage) => another(RELOADS, store);

/** The service could not be started again: the page says so instead of offering a reload. */
const [startFailed, setStartFailed] = createSignal(false);
export { startFailed };
export const couldNotStart = "Momotus could not start. Close the app and open it again.";

/** Starts the service again for a new token, then reloads with it; without Luna goes back to the packaged stub.
 * At most 3 times in a minute, so a service that never comes up does not spin; past that, `startFailed`. */
export function relaunch(store: Storage = sessionStorage) {
  if (!another(RELAUNCHES, store)) return void setStartFailed(true);
  // `identifier` is "<app id> <process id>" (LG's webOS.js fetchAppId).
  const app = (window as unknown as { PalmSystem?: { identifier?: string } }).PalmSystem?.identifier?.split(" ")[0];
  const stub = () => {
    const back = store.getItem(STUB);
    if (back) location.replace(back);
    else setStartFailed(true);
  };
  if (!app) return stub();
  luna(`luna://${app}.service/start`, {}, (reply) => {
    if (!reply?.returnValue || typeof reply.token !== "string") return stub();
    store.setItem(TOKEN, reply.token);
    location.reload();
  });
}
