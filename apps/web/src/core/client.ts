/** The app's one bridge connection and the live views built on it. Each view
 * is a Solid store fed by the core's snapshot and pushes, so components read
 * plain fields and re-render only what changed (see plans/core-protocol.md). */

import { batch, createSignal, onCleanup } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { Bridge, BridgeError, ipcTransport, shellPost, webSocketTransport } from "./bridge";
import { applyDiffs } from "./diff";
import { isPrivateLink } from "../lib/sync";
import { mediaSource } from "../player/hls";
import { failureWords, kidsCopy } from "../lib/words";
import type { DiscoverSnapshot } from "./generated/DiscoverSnapshot";
import type { Episode } from "./generated/Episode";
import type { Hero } from "./generated/Hero";
import type { HomeOffline } from "./generated/HomeOffline";
import type { HomeRow } from "./generated/HomeRow";
import type { HomeSnapshot } from "./generated/HomeSnapshot";
import type { HomeUpdate } from "./generated/HomeUpdate";
import type { LibrarySnapshot } from "./generated/LibrarySnapshot";
import type { Prefs } from "./generated/Prefs";
import { commandQueue } from "./command-queue";
import type { SourceKind } from "./generated/SourceKind";
import type { Tile } from "./generated/Tile";
import type { TitleHeader } from "./generated/TitleHeader";
import type { TitleSnapshot } from "./generated/TitleSnapshot";
import type { TitleUpdate } from "./generated/TitleUpdate";
import type { VersionsSnapshot } from "./generated/VersionsSnapshot";
import type { Network } from "./generated/Network";
import { webosDeviceCaps } from "./webos-caps";
import { webosTransport } from "./webos-transport";
import { isWebOS } from "../lib/webos";

export const bridgeUrl: string = import.meta.env.VITE_MQ_BRIDGE ?? `ws://${location.hostname || "127.0.0.1"}:7777/ws`;

// In a desktop shell the core runs in process and answers through the shell's
// message handler (plans/m3-desktop.md section 3); in a browser it is the
// development WebSocket.
const shellHandler = window.__MQ_SHELL__ ? shellPost() : null;
// The shell's player capabilities, from its `ready`, in the core's names.
let shellCaps: DeviceCaps | null = null;
// On webOS the service serves this page and its socket on one origin (a walk build has its own port).
const webosSocket = `ws://${location.host}/ws`;
const transport = shellHandler
  ? ipcTransport(shellHandler, (r) => (shellCaps = asCaps(r.caps)))
  : isWebOS()
    ? webosTransport(webosSocket)
    : webSocketTransport(bridgeUrl);
export const bridge = new Bridge(transport);
/** Whether the sync record may travel this link: the shell's IPC, or a socket that is encrypted or stays on this machine. */
export const privateLink = shellHandler !== null || isPrivateLink(isWebOS() ? webosSocket : bridgeUrl);

export type Link = "connecting" | "open" | "closed";
const [link, setLink] = createSignal<Link>("connecting");
export { link };
transport.onClose(() => setLink("closed"));

// The core pauses background work while the app is hidden and retries
// network work when the connection returns.
const lifecycle = (event: string, fields: Record<string, unknown> = {}) => void bridge.request("lifecycle", { event, ...fields }).catch(() => undefined);

type Connection = EventTarget & { type?: string };
const connection = (navigator as Navigator & { connection?: Connection }).connection;

/** The network type speed is measured for (plans/m13-stream-choice.md 4.6);
 * most desktop browsers do not say, so "other". */
export function networkType(): Network {
  const type = connection?.type;
  return type === "wifi" || type === "ethernet" || type === "cellular" ? type : "other";
}
// Back in front, the core may lock a PIN profile that was away over 30
// minutes, and it binds the stream server again when the system took its
// port: then `serverMoved`, and a loaded playback opens again (the player
// watches `serverMoves`).
const [serverMoves, setServerMoves] = createSignal(0);
export { serverMoves };
const foreground = () =>
  void bridge
    .request<{ serverMoved?: boolean }>("lifecycle", { event: "foreground" })
    .then((r) => r?.serverMoved && setServerMoves((n) => n + 1))
    .catch(() => undefined);
document.addEventListener("visibilitychange", () => (document.hidden ? lifecycle("background") : foreground()));
// Leaving the page: the core pauses what it scrobbles (Trakt) before the tab goes.
window.addEventListener("pagehide", () => lifecycle("background"));
// The network as downloads read it (plans/offline-downloads.md 4.13): `none` while the browser is
// offline, so downloads wait and the shell's banner says so (3.5). A browser has no cost flag:
// `metered` and `lowData` are always false (Download on Wi-Fi only is not offered here).
const [online, setOnline] = createSignal(navigator.onLine);
export { online };
const networkChanged = () => {
  setOnline(navigator.onLine);
  lifecycle("networkChanged", { network: navigator.onLine ? networkType() : "none", metered: false, lowData: false });
};
window.addEventListener("online", networkChanged);
window.addEventListener("offline", networkChanged);
connection?.addEventListener("change", networkChanged);
// Told at load, online or not: the core runs no download before it hears the network.
networkChanged();

/** A failed view: `reason` is what the screen says (`reasonOf`), `raw` the core's own text, for Details only. */
export type ViewStatus = { state: "opening" } | { state: "open" } | { state: "failed"; reason: string; raw?: string };

/** This browser's language as the core's codes begin ("nb" for "nb-NO"). */
export const deviceLanguage = () => (navigator.language || "en").split("-")[0]?.toLowerCase() || "en";

/** The link to the core is gone: plain words, never its address (QA r1, Q5 row 39). */
export const lostWords = "Lost the connection to Momotus. Start it again, then reload.";

/** What a screen says for a failure: its kind's words (apps/shared/error-words.json), never the raw text. */
export const reasonOf = (e: unknown) =>
  e instanceof Error && e.message === "connection closed"
    ? lostWords
    : e instanceof BridgeError && e.code === "incognito"
      ? "Not available in Incognito."
      : failureWords(e);

/** A view that could not open: the words, and the core's raw text behind Details (none when it is its own sentence). */
const failedStatus = (e: unknown): ViewStatus => {
  const reason = reasonOf(e);
  const raw = e instanceof Error ? e.message : String(e);
  return raw === reason ? { state: "failed", reason } : { state: "failed", reason, raw };
};

/** A failed view's raw text, for Details. */
export const rawOf = (s: ViewStatus | undefined) => (s?.state === "failed" ? s.raw : undefined);

/** Work from before a profile switch or a lock (`switched`): its screen is gone, so nothing shows it. */
export const isSwitched = (e: unknown) => e instanceof BridgeError && e.code === "switched";
/** `installAddon` refused a link whose settings carry a sign-in until the user confirms it is theirs. */
export const isTokenInLink = (e: unknown) => e instanceof BridgeError && e.code === "tokenInLink";
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

/** The open views of one title (title, versions, playback), by subscription: the core closes them when the title
 * leaves a kids list (plans/m11-kids.md 8), and each then fails with that line. */
const ofTitle = new Map<number, { item: string; fail: () => void }>();
const [takenOff, setTakenOff] = createSignal<{ items: string[] } | null>(null);
/** The titles whose open views the core last closed for a kids session: their screens leave. */
export { takenOff };
bridge.onClosed((subs) => {
  const items = subs.flatMap((n) => {
    const v = ofTitle.get(n);
    if (!v) return [];
    ofTitle.delete(n);
    v.fail();
    return [v.item];
  });
  if (items.length) setTakenOff({ items });
});
const closedStatus: ViewStatus = { state: "failed", reason: kidsCopy.takenOff };
/** Closes a subscription once its opening settled; a failed or lost one has nothing to close. */
const closeWhenOpen = (opening: Promise<Subscription | null>) =>
  opening.then((s) => s?.close()).catch(() => undefined);

/** Opens a record view (pushes are whole snapshots; the last one wins). It
 * closes with the calling component unless `keep` is set. Array items are
 * matched by `key` across pushes (`id` unless a view's items repeat it);
 * `ids` gives a snapshot whose lists have none their ids first (lib/ids.ts). */
export function liveView<S>(view: string, params: Record<string, unknown> = {}, keep = false, key = "id", ids?: (s: S) => S): LiveView<S> {
  const [state, setState] = createStore<Live<S>>({ status: { state: "opening" }, data: null });
  const [sub, setSub] = createSignal<number | null>(null);
  // A command sent while the view opens (a query typed the moment the search
  // palette shows) waits for it, in order; once open, commands go at once.
  const commands = commandQueue<object>();
  let closing: Promise<void> | undefined;
  const apply = (s: S) => setState("data", reconcile((ids ? ids(s) : s) as never, { merge: true, key }));
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
        commands.open((c) => s.command(c));
        if (typeof params.itemId === "string")
          ofTitle.set(s.sub, {
            item: params.itemId,
            fail: () => {
              commands.drop();
              setState("status", closedStatus);
            },
          });
      } else commands.drop();
      return s;
    })
    .catch((e) => {
      setState("status", failedStatus(e));
      commands.drop();
      return null;
    });
  const close = () => {
    commands.drop();
    ofTitle.delete(sub() ?? -1);
    return (closing ??= closeWhenOpen(opening));
  };
  if (!keep) onCleanup(() => void close());
  return { state, sub, command: commands.send, close };
}

export interface HomeState {
  status: ViewStatus;
  /** The hero carousel's pages, in the core's order. */
  heroes: Hero[];
  /** No network and downloads that play (plans/offline-downloads.md 3.5): Home's banner. */
  offline: HomeOffline | null;
  rows: HomeRow[];
}

let home: HomeState | undefined;
let homeOpening: Promise<Subscription | null> | undefined;

/** Home stays subscribed for the app's lifetime so Back lands on the same
 * rows, scroll offsets and focus without a reload. */
export function homeView(): HomeState {
  if (home) return home;
  const [state, setState] = createStore<HomeState>({ status: { state: "opening" }, heroes: [], offline: null, rows: [] });
  home = state;
  homeOpening = bridge
    .subscribe<HomeSnapshot, HomeUpdate>("home", {}, {
      snapshot: (snapshot) => setState({ status: { state: "open" }, heroes: snapshot.header.heroes, offline: snapshot.header.offline, rows: snapshot.rows }),
      updates: (updates) =>
        batch(() => {
          for (const u of updates) {
            if (u.kind === "header") {
              setState("heroes", reconcile(u.header.heroes));
              setState("offline", u.header.offline);
            }
            else setState("rows", reconcile(applyDiffs(state.rows, u.diffs), { key: "rowId" }));
          }
        }),
    })
    .then((s) => {
      setLink("open");
      return s;
    })
    .catch((e) => {
      setState("status", failedStatus(e));
      return null;
    });
  return state;
}

/** A failed Home row's Try again: the core asks that one catalog again (the
 * row turns loading, then shows the answer or fails anew), not all of Home. */
export const retryHomeRow = (rowId: string) => homeOpening?.then((s) => s?.command({ type: "retryRow", rowId }));

export interface TitleState {
  status: ViewStatus;
  header: TitleHeader | null;
  episodes: Episode[];
  /** "More like this": empty until it lands, and for a kids profile. */
  similar: Tile[];
}

export interface TitleView {
  state: TitleState;
  command(c: object): Promise<void>;
}

/** A title view for the lifetime of the calling component. */
export function titleView(kind: string, id: string): TitleView {
  const [state, setState] = createStore<TitleState>({ status: { state: "opening" }, header: null, episodes: [], similar: [] });
  // A command sent in the moment between the snapshot and the subscription's answer (`titleVisible` as the
  // page first shows) waits for it, in order, as liveView's do; once closed, commands are dropped.
  const commands = commandQueue<object>();
  let close: (() => Promise<void>) | undefined;
  let disposed = false;
  let opened: number | undefined;
  onCleanup(() => {
    disposed = true;
    commands.drop();
    ofTitle.delete(opened ?? -1);
    void close?.();
  });
  bridge
    .subscribe<TitleSnapshot, TitleUpdate>("title", { kind, id }, {
      snapshot: (snapshot) =>
        setState({ status: { state: "open" }, header: snapshot.header, episodes: snapshot.episodes, similar: snapshot.similar ?? [] }),
      updates: (updates) =>
        batch(() => {
          for (const u of updates) {
            if (u.kind === "header") setState("header", reconcile(u.header));
            else if (u.kind === "episodes") setState("episodes", reconcile(applyDiffs(state.episodes, u.diffs), { key: "videoId" }));
            else if (u.kind === "similar") setState("similar", reconcile(u.tiles, { key: "id" }));
            // A kind a newer core sends is left alone: the page keeps working.
          }
        }),
    })
    .then((sub) => {
      setLink("open");
      if (disposed) return void sub.close();
      close = sub.close;
      commands.open((c) => sub.command(c));
      opened = sub.sub;
      ofTitle.set(sub.sub, {
        item: id,
        fail: () => {
          commands.drop();
          setState("status", closedStatus);
        },
      });
    })
    .catch((e) => {
      setState("status", failedStatus(e));
      commands.drop();
    });
  return { state, command: commands.send };
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
  // HLS: natively (Safari, Chrome 142+), else through hls.js where Media
  // Source Extensions exist (player/hls.ts). DASH comes as HLS from the core.
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
  const webos = webosDeviceCaps(deviceLanguage());
  if (webos) return webos;
  // The display, never the window: a small window on a 1080p screen still
  // shows 1080p when it goes full screen. Taller than 720 lines counts as 1080.
  const lines = screen.height * (window.devicePixelRatio || 1);
  return {
    displayHeight: lines >= 2000 ? 2160 : lines > 720 ? 1080 : 720,
    hdr: matchMedia("(dynamic-range: high)").matches ? ["hdr10", "hlg"] : [],
    decoders: DECODERS.filter(([, type]) => decodes(type)).map(([name]) => name),
    tenBitHevc: decodes('video/mp4; codecs="hvc1.2.4.L120.90"'),
    audioDecoders: AUDIO.filter(([, type]) => plays(type)).map(([name]) => name),
    containers: CONTAINERS.filter(([name, type]) => plays(type) || (name === "m3u8" && mediaSource())).map(([name]) => name),
    passthrough: [],
    hasFallbackEngine: false,
    language: deviceLanguage(),
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
  /** The title as the app knows it, for the bitrate and the year check
   * (plans/m13-stream-choice.md 4.3); the core reads the meta for the rest. */
  runtime?: string;
  releaseInfo?: string;
  released?: string;
  episodesInSeason?: number;
  /** `download`: the versions to keep on this device (plans/offline-downloads.md 4.5). */
  purpose?: "download";
  /** The title the video belongs to: a kids profile is refused without it. */
  itemId: string;
}

let versions: { key: string; view: LiveView<VersionsSnapshot> } | null = null;

/** One Versions view at a time, shared by the Versions screen and the player:
 * playback takes rows by key from the same open view, so it outlives both
 * screens until a different video asks for versions. */
export function versionsView(p: VersionsParams): LiveView<VersionsSnapshot> {
  const key = `${p.kind}|${p.videoId}|${p.bingeGroup ?? ""}|${p.purpose ?? ""}`;
  if (versions?.key === key && versions.view.state.status.state !== "failed") return versions.view;
  versions?.view.close();
  // Rows merge by their `key`, so a row keeps its element (and the focus) when
  // a late addon or a re-rank moves it.
  const view = liveView<VersionsSnapshot>("versions", { ...p, ...displayCaps(), network: networkType() }, true, "key");
  kindAsked = false;
  versions = { key, view };
  return view;
}

// A kind was asked of the kept view since every kind was last listed: its answer may not be in yet.
let kindAsked = false;

/** Asks the kept Versions view for one kind of version, or every one (null): `setKind`, answered with the kind's own
 * card, rows and sections (plans/plugins-and-marketplace.md 4.2). */
export function askKind(kind: SourceKind | null): Promise<void> | undefined {
  kindAsked = kind !== null;
  return versions?.view.command({ type: "setKind", kind });
}

/** A kind holds while the video's Versions page or player is open; any other page lists every kind again, so the
 * title's Play plays the core's pick. */
export function listEveryKind() {
  if (kindAsked || versions?.view.state.data?.selectedKind) void askKind(null);
}

let prefs: LiveView<Prefs> | undefined;
let discover: LiveView<DiscoverSnapshot> | undefined;
let library: LiveView<LibrarySnapshot> | undefined;

/** Discover, kept open for the app's life as Home is: Back lands on the
 * viewer's type, catalog, genre and loaded pages. One that failed opens anew. */
export function discoverView(): LiveView<DiscoverSnapshot> {
  if (!discover || discover.state.status.state === "failed") discover = liveView<DiscoverSnapshot>("discover", {}, true);
  return discover;
}

/** Library, kept open for the app's life: Back lands on the viewer's filter and sort. */
export function libraryView(): LiveView<LibrarySnapshot> {
  if (!library || library.state.status.state === "failed") library = liveView<LibrarySnapshot>("library", {}, true);
  return library;
}

/** Before a profile switch or after a lock: the views this module keeps for
 * the app's life belong to the old profile, so they close and reopen fresh.
 * Settles once the core closed each of them, including one still opening. */
export async function closeProfileViews() {
  const closing = [homeOpening && closeWhenOpen(homeOpening), versions?.view.close(), prefs?.close(), discover?.close(), library?.close()];
  home = undefined;
  homeOpening = undefined;
  versions = null;
  prefs = undefined;
  discover = undefined;
  library = undefined;
  await Promise.all(closing);
}

/** The profile's preferences, kept open for the app's lifetime. */
export function prefsView(): LiveView<Prefs> {
  prefs ??= liveView<Prefs>("settings", {}, true);
  return prefs;
}
