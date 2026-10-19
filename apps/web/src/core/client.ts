/** The app's one bridge connection and the live views built on it. Each view
 * is a Solid store fed by the core's snapshot and pushes, so components read
 * plain fields and re-render only what changed (see plans/core-protocol.md). */

import { batch, createSignal, onCleanup } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { Bridge, BridgeError, ipcTransport, shellPost, webSocketTransport } from "./bridge";
import { applyDiffs } from "./diff";
import { isPrivateLink } from "../lib/sync";
import type { Episode } from "./generated/Episode";
import type { Hero } from "./generated/Hero";
import type { HomeRow } from "./generated/HomeRow";
import type { HomeSnapshot } from "./generated/HomeSnapshot";
import type { HomeUpdate } from "./generated/HomeUpdate";
import type { Prefs } from "./generated/Prefs";
import type { TitleHeader } from "./generated/TitleHeader";
import type { TitleSnapshot } from "./generated/TitleSnapshot";
import type { TitleUpdate } from "./generated/TitleUpdate";
import type { VersionsSnapshot } from "./generated/VersionsSnapshot";

export const bridgeUrl: string = import.meta.env.VITE_MQ_BRIDGE ?? `ws://${location.hostname || "127.0.0.1"}:7777/ws`;

// In a desktop shell the core runs in process and answers through the shell's
// message handler (plans/m3-desktop.md section 3); in a browser it is the
// development WebSocket.
const shellHandler = window.__MQ_SHELL__ ? shellPost() : null;
// The shell's player capabilities, from its `ready`, in the core's names.
let shellCaps: DeviceCaps | null = null;
const transport = shellHandler ? ipcTransport(shellHandler, (r) => (shellCaps = asCaps(r.caps))) : webSocketTransport(bridgeUrl);
export const bridge = new Bridge(transport);
/** Whether the sync record may travel this link: the shell's IPC, or a socket that is encrypted or stays on this machine. */
export const privateLink = shellHandler !== null || isPrivateLink(bridgeUrl);

export type Link = "connecting" | "open" | "closed";
const [link, setLink] = createSignal<Link>("connecting");
export { link };
transport.onClose(() => setLink("closed"));

// The core pauses background work while the app is hidden and retries
// network work when the connection returns.
const lifecycle = (event: string) => void bridge.request("lifecycle", { event }).catch(() => undefined);
document.addEventListener("visibilitychange", () => lifecycle(document.hidden ? "background" : "foreground"));
window.addEventListener("online", () => lifecycle("networkChanged"));

export type ViewStatus ={ state: "opening" } | { state: "open" } | { state: "failed"; reason: string };

export const reasonOf = (e: unknown) =>
  e instanceof Error && e.message === "connection closed"
    ? `Cannot reach the core at ${bridgeUrl}. Start it with: cargo run -p mq-bridge`
    : String(e instanceof Error ? e.message : e);

/** Work from before a profile switch or a lock (`switched`): its screen is gone, so nothing shows it. */
export const isSwitched = (e: unknown) => e instanceof BridgeError && e.code === "switched";
window.addEventListener("unhandledrejection", (e) => {
  if (isSwitched(e.reason)) e.preventDefault();
});

/** Runs a one-shot op such as `installAddon` or `setPrefs`. */
export async function op<T = unknown>(name: string, fields: Record<string, unknown> = {}): Promise<T> {
  const result = await bridge.request<T>(name, fields);
  setLink("open");
  return result;
}

export interface Live<S> {
  status: ViewStatus;
  data: S | null;
}

export interface LiveView<S> {
  state: Live<S>;
  sub: () => number | null;
  command(c: object): Promise<void>;
  /** Settles once the core closed the view, even one still opening. */
  close(): Promise<void>;
}

type Subscription = Awaited<ReturnType<typeof bridge.subscribe>>;
/** Closes a subscription once its opening settled; a failed or lost one has nothing to close. */
const closeWhenOpen = (opening: Promise<Subscription | null>) =>
  opening.then((s) => s?.close()).catch(() => undefined);

/** Opens a record view (pushes are whole snapshots; the last one wins). It
 * closes with the calling component unless `keep` is set. Array items are
 * matched by `key` across pushes (`id` unless a view's items repeat it). */
export function liveView<S>(view: string, params: Record<string, unknown> = {}, keep = false, key = "id"): LiveView<S> {
  const [state, setState] = createStore<Live<S>>({ status: { state: "opening" }, data: null });
  const [sub, setSub] = createSignal<number | null>(null);
  let command: (c: object) => Promise<void> = async () => undefined;
  let closing: Promise<void> | undefined;
  const apply = (s: S) => setState("data", reconcile(s as never, { merge: true, key }));
  const opening = bridge
    .subscribe<S, S>(view, params, {
      snapshot: (s) =>
        batch(() => {
          apply(s);
          setState("status", { state: "open" });
        }),
      updates: (updates) => {
        const last = updates[updates.length - 1];
        if (last !== undefined) apply(last);
      },
    })
    .then((s) => {
      setLink("open");
      if (!closing) {
        setSub(s.sub);
        command = s.command;
      }
      return s;
    })
    .catch((e) => {
      setState("status", { state: "failed", reason: reasonOf(e) });
      return null;
    });
  const close = () => (closing ??= closeWhenOpen(opening));
  if (!keep) onCleanup(() => void close());
  return { state, sub, command: (c) => command(c), close };
}

export interface HomeState {
  status: ViewStatus;
  hero: Hero | null;
  rows: HomeRow[];
}

let home: HomeState | undefined;
let homeOpening: Promise<Subscription | null> | undefined;

/** Home stays subscribed for the app's lifetime so Back lands on the same
 * rows, scroll offsets and focus without a reload. */
export function homeView(): HomeState {
  if (home) return home;
  const [state, setState] = createStore<HomeState>({ status: { state: "opening" }, hero: null, rows: [] });
  home = state;
  homeOpening = bridge
    .subscribe<HomeSnapshot, HomeUpdate>("home", {}, {
      snapshot: (snapshot) => setState({ status: { state: "open" }, hero: snapshot.header.hero, rows: snapshot.rows }),
      updates: (updates) =>
        batch(() => {
          for (const u of updates) {
            if (u.kind === "header") setState("hero", reconcile(u.header.hero));
            else setState("rows", reconcile(applyDiffs(state.rows, u.diffs), { key: "rowId" }));
          }
        }),
    })
    .then((s) => {
      setLink("open");
      return s;
    })
    .catch((e) => {
      setState("status", { state: "failed", reason: reasonOf(e) });
      return null;
    });
  return state;
}

export interface TitleState {
  status: ViewStatus;
  header: TitleHeader | null;
  episodes: Episode[];
}

export interface TitleView {
  state: TitleState;
  command(c: object): Promise<void>;
}

/** A title view for the lifetime of the calling component. */
export function titleView(kind: string, id: string): TitleView {
  const [state, setState] = createStore<TitleState>({ status: { state: "opening" }, header: null, episodes: [] });
  let command: (c: object) => Promise<void> = async () => undefined;
  let close: (() => Promise<void>) | undefined;
  let disposed = false;
  onCleanup(() => {
    disposed = true;
    void close?.();
  });
  bridge
    .subscribe<TitleSnapshot, TitleUpdate>("title", { kind, id }, {
      snapshot: (snapshot) => setState({ status: { state: "open" }, header: snapshot.header, episodes: snapshot.episodes }),
      updates: (updates) =>
        batch(() => {
          for (const u of updates) {
            if (u.kind === "header") setState("header", reconcile(u.header));
            else setState("episodes", reconcile(applyDiffs(state.episodes, u.diffs), { key: "videoId" }));
          }
        }),
    })
    .then((sub) => {
      setLink("open");
      if (disposed) return void sub.close();
      close = sub.close;
      command = sub.command;
    })
    .catch((e) => setState("status", { state: "failed", reason: reasonOf(e) }));
  return { state, command: (c) => command(c) };
}

export interface DeviceCaps {
  displayHeight: number;
  hdr: string[];
  decoders: string[];
  tenBitHevc: boolean;
  audioDecoders: string[];
  containers: string[];
  passthrough: string[];
  hasFallbackEngine: boolean;
  /** ISO 639-1; the core prefers it, then English, until the user picks. */
  language: string;
}

// Audio names as the core expects them. DTS and TrueHD are never decoded by
// a browser, so they are not probed.
const AUDIO: [name: string, type: string][] = [
  ["aac", 'audio/mp4; codecs="mp4a.40.2"'],
  ["opus", 'audio/webm; codecs="opus"'],
  ["mp3", "audio/mpeg"],
  ["flac", "audio/flac"],
  ["ac3", 'audio/mp4; codecs="ac-3"'],
  ["eac3", 'audio/mp4; codecs="ec-3"'],
];

const CONTAINERS: [name: string, type: string][] = [
  ["mp4", "video/mp4"],
  ["webm", "video/webm"],
  // HLS where the browser plays it natively (Safari; others answer "").
  ["m3u8", "application/vnd.apple.mpegurl"],
];

// Codec names as the core's ranking expects them, with a probe for each.
const DECODERS: [name: string, type: string][] = [
  ["h264", 'video/mp4; codecs="avc1.640028"'],
  ["h265", 'video/mp4; codecs="hvc1.1.6.L120.90"'],
  ["av1", 'video/mp4; codecs="av01.0.08M.08"'],
  ["vp9", 'video/webm; codecs="vp9"'],
];

const decodes = (type: string) =>
  (typeof MediaSource !== "undefined" && MediaSource.isTypeSupported(type)) || document.createElement("video").canPlayType(type) === "probably";
// Plain types without codecs answer "maybe" at best.
const plays = (type: string) => decodes(type) || document.createElement("video").canPlayType(type) !== "";

/** Shell capabilities only when every field has the core's shape; anything
 * else falls back to what the browser reports. */
function asCaps(x: unknown): DeviceCaps | null {
  const c = x as Partial<DeviceCaps> | null;
  const strings = (v: unknown) => Array.isArray(v) && v.every((s) => typeof s === "string");
  const ok =
    !!c &&
    typeof c.displayHeight === "number" &&
    strings(c.hdr) &&
    strings(c.decoders) &&
    strings(c.audioDecoders) &&
    strings(c.containers) &&
    strings(c.passthrough) &&
    typeof c.tenBitHevc === "boolean" &&
    typeof c.hasFallbackEngine === "boolean" &&
    typeof c.language === "string";
  return ok ? (c as DeviceCaps) : null;
}

/** What this device can show and decode, so Versions ranks for it: the
 * shell's player when there is one, else this browser. A web player decodes
 * audio itself (no passthrough) and has no second engine. */
export function displayCaps(): DeviceCaps {
  if (shellCaps) return shellCaps;
  // The display, never the window: a small window on a 1080p screen still
  // shows 1080p when it goes full screen. Taller than 720 lines counts as 1080.
  const lines = screen.height * (window.devicePixelRatio || 1);
  return {
    displayHeight: lines >= 2000 ? 2160 : lines > 720 ? 1080 : 720,
    hdr: matchMedia("(dynamic-range: high)").matches ? ["hdr10", "hlg"] : [],
    decoders: DECODERS.filter(([, type]) => decodes(type)).map(([name]) => name),
    tenBitHevc: decodes('video/mp4; codecs="hvc1.2.4.L120.90"'),
    audioDecoders: AUDIO.filter(([, type]) => plays(type)).map(([name]) => name),
    containers: CONTAINERS.filter(([, type]) => plays(type)).map(([name]) => name),
    passthrough: [],
    hasFallbackEngine: false,
    language: (navigator.language || "en").slice(0, 2).toLowerCase(),
  };
}

/** Round trip to the core in milliseconds (`ping` answers from the request
 * loop at once, so this measures the link, not the network). */
export async function ping(): Promise<number> {
  const t0 = performance.now();
  await op("ping");
  return Math.round(performance.now() - t0);
}

export interface VersionsParams {
  kind: string;
  videoId: string;
  title: string;
  season?: number;
  episode?: number;
  bingeGroup?: string;
}

let versions: { key: string; view: LiveView<VersionsSnapshot> } | null = null;

/** One Versions view at a time, shared by the Versions screen and the player:
 * playback takes rows by key from the same open view, so it outlives both
 * screens until a different video asks for versions. */
export function versionsView(p: VersionsParams): LiveView<VersionsSnapshot> {
  const key = `${p.kind}|${p.videoId}|${p.bingeGroup ?? ""}`;
  if (versions?.key === key && versions.view.state.status.state !== "failed") return versions.view;
  versions?.view.close();
  const view = liveView<VersionsSnapshot>("versions", { ...p, ...displayCaps() }, true);
  versions = { key, view };
  return view;
}

let prefs: LiveView<Prefs> | undefined;

/** Before a profile switch or after a lock: the views this module keeps for
 * the app's life belong to the old profile, so they close and reopen fresh.
 * Settles once the core closed each of them, including one still opening. */
export async function closeProfileViews() {
  const closing = [homeOpening && closeWhenOpen(homeOpening), versions?.view.close(), prefs?.close()];
  home = undefined;
  homeOpening = undefined;
  versions = null;
  prefs = undefined;
  await Promise.all(closing);
}

/** The profile's preferences, kept open for the app's lifetime. */
export function prefsView(): LiveView<Prefs> {
  prefs ??= liveView<Prefs>("settings", {}, true);
  return prefs;
}
