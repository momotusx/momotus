/** The brokered sign-in on the web (plans/accounts.md 4.2): the provider page opens in a window
 * opened in the click itself (one opened after an await is a pop-up the browser blocks), comes
 * back to this app's own `/auth`, and that page hands the fragment to this window over a
 * same-origin BroadcastChannel only (main.tsx), never `postMessage` to `*`. The core's `signIn`
 * view keeps the verifier; the ticket a sign-in brings stays in this module's memory, never in
 * the URL or storage. */

import { createSignal } from "solid-js";
import { sameRelay } from "../lib/account";
import { bridge, op } from "./client";
import type { SignInSnapshot } from "./generated/SignInSnapshot";
import { rememberedServer } from "./sync";

export const AUTH_CHANNEL = "mq-auth";

/** How a sign-in ended: an account found, none yet, a revoke's receipt, or the core's code. */
export type SignInEnd = Extract<SignInSnapshot, { phase: "found" | "new" | "receipt" | "failed" }>;

/** The sync service sign-in goes to: the one this device used, else the build's hosted default
 * (`VITE_MARQUEE_SYNC_SERVER`, the web's `MARQUEE_SYNC_SERVER`, plans/accounts.md 9.2; unset keeps
 * today's onboarding). */
export const signInServer = (): string => rememberedServer() || ((import.meta.env.VITE_MARQUEE_SYNC_SERVER as string | undefined) ?? "");

/** The providers a sync service offers (none: no sign-in there, only the pairing and kit doors). */
export const relayProviders = async (server: string) => (await op<{ providers: string[] }>("relayInfo", { server })).providers;

/** In the click handler, before any await. Null when the browser blocked it. */
export const openSignInWindow = (): Window | null => window.open("about:blank", "_blank", "popup,width=520,height=720");

export interface SignInRun {
  done: Promise<SignInEnd>;
  /** Back on this window with no return from the provider window: "Didn't finish signing in?".
   * The flow keeps waiting, so a return that comes later still finishes it. */
  stalled: () => boolean;
  /** Ends the flow as cancelled and closes its window. */
  cancel: () => void;
}

/** Runs one sign-in (or Apple's revoke) in `win`, opened by `openSignInWindow`. */
export function runSignIn(win: Window | null, server: string, provider: string, revoke = false): SignInRun {
  const [stalled, setStalled] = createSignal(false);
  if (!win) return { done: Promise.resolve({ phase: "failed", code: "popupBlocked" }), stalled, cancel: () => undefined };
  let settle: (end: SignInEnd) => void = () => undefined;
  const done = new Promise<SignInEnd>((ok) => (settle = ok));
  let ended = false;
  let shown = false;
  let returned = false;
  let view: Awaited<ReturnType<typeof bridge.subscribe>> | null = null;
  const channel = new BroadcastChannel(AUTH_CHANNEL);
  // Coming back to this window is when "Didn't finish signing in?" is asked: a provider page
  // that cuts the opener off (COOP) makes the window look closed while it is still open.
  let later: ReturnType<typeof setTimeout> | undefined;
  const back = () => {
    clearTimeout(later);
    later = setTimeout(() => setStalled(shown && !returned && !ended), 1500);
  };
  const end = (e: SignInEnd) => {
    if (ended) return;
    ended = true;
    setStalled(false);
    clearTimeout(later);
    channel.close();
    removeEventListener("focus", back);
    void view?.close();
    if (!win.closed) win.close();
    settle(e);
  };
  const seen = (s: SignInSnapshot) => {
    if (s.phase === "browser" && !shown) {
      shown = true;
      win.location.href = s.url;
    } else if (s.phase !== "starting" && s.phase !== "browser" && s.phase !== "waiting") end(s);
  };
  channel.onmessage = (e: MessageEvent) => {
    const fragment = typeof e.data === "string" ? e.data : "";
    returned = true;
    setStalled(false);
    // The core ignores a return it did not start; one from another sync service ends this one.
    if (!sameRelay(fragment, server)) return end({ phase: "failed", code: "otherRelay" });
    void view?.command({ type: "returned", fragment }).catch(() => undefined);
  };
  addEventListener("focus", back);
  bridge
    .subscribe<SignInSnapshot, SignInSnapshot>(revoke ? "appleRevoke" : "signIn", { server, provider, return: `${location.origin}/auth` }, {
      snapshot: seen,
      updates: (all) => {
        const last = all[all.length - 1];
        if (last) seen(last);
      },
    })
    .then(
      (opened) => {
        view = opened;
        if (ended) void opened.close();
      },
      () => end({ phase: "failed", code: "unreachable" }),
    );
  return { done, stalled, cancel: () => end({ phase: "failed", code: "cancelled" }) };
}

/** A sign-in's result held for the next screen: in memory only, gone with the tab. */
export interface Held {
  server: string;
  provider: string;
  ticket: string;
  account: string | null;
}
const [held, setHeld] = createSignal<Held | null>(null);
export { held, setHeld };
