/** The app's one bridge connection and the live views built on it. Each view
 * is a Solid store fed by the core's snapshot and pushes, so components read
 * plain fields and re-render only what changed (see plans/core-protocol.md). */

import { batch, createSignal, onCleanup } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { Bridge, webSocketTransport } from "./bridge";
import { applyDiffs } from "./diff";
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

const transport = webSocketTransport(bridgeUrl);
export const bridge = new Bridge(transport);

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
  close(): void;
}

/** Opens a record view (pushes are whole snapshots; the last one wins). It
 * closes with the calling component unless `keep` is set. */
export function liveView<S>(view: string, params: Record<string, unknown> = {}, keep = false): LiveView<S> {
  const [state, setState] = createStore<Live<S>>({ status: { state: "opening" }, data: null });
  const [sub, setSub] = createSignal<number | null>(null);
  let command: (c: object) => Promise<void> = async () => undefined;
  let closeSub: (() => Promise<void>) | undefined;
  let closed = false;
  const apply = (s: S) => setState("data", reconcile(s as never, { merge: true }));
  bridge
    .subscribe<S, S>(view, params, (updates) => {
      const last = updates[updates.length - 1];
      if (last !== undefined) apply(last);
    })
    .then((s) => {
      setLink("open");
      if (closed) return void s.close();
      setSub(s.sub);
      closeSub = s.close;
      command = s.command;
      batch(() => {
        apply(s.snapshot);
        setState("status", { state: "open" });
      });
    })
    .catch((e) => setState("status", { state: "failed", reason: reasonOf(e) }));
  const close = () => {
    closed = true;
    void closeSub?.();
  };
  if (!keep) onCleanup(close);
  return { state, sub, command: (c) => command(c), close };
}

export interface HomeState {
  status: ViewStatus;
  hero: Hero | null;
  rows: HomeRow[];
}

let home: HomeState | undefined;

/** Home stays subscribed for the app's lifetime so Back lands on the same
 * rows, scroll offsets and focus without a reload. */
export function homeView(): HomeState {
  if (home) return home;
  const [state, setState] = createStore<HomeState>({ status: { state: "opening" }, hero: null, rows: [] });
  home = state;
  bridge
    .subscribe<HomeSnapshot, HomeUpdate>("home", {}, (updates) =>
      batch(() => {
        for (const u of updates) {
          if (u.kind === "header") setState("hero", reconcile(u.header.hero));
          else setState("rows", reconcile(applyDiffs(state.rows, u.diffs), { key: "rowId" }));
        }
      }),
    )
    .then(({ snapshot }) => {
      setLink("open");
      setState({ status: { state: "open" }, hero: snapshot.header.hero, rows: snapshot.rows });
    })
    .catch((e) => setState("status", { state: "failed", reason: reasonOf(e) }));
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
    .subscribe<TitleSnapshot, TitleUpdate>("title", { kind, id }, (updates) =>
      batch(() => {
        for (const u of updates) {
          if (u.kind === "header") setState("header", reconcile(u.header));
          else setState("episodes", reconcile(applyDiffs(state.episodes, u.diffs), { key: "videoId" }));
        }
      }),
    )
    .then((sub) => {
      setLink("open");
      if (disposed) return void sub.close();
      close = sub.close;
      command = sub.command;
      setState({ status: { state: "open" }, header: sub.snapshot.header, episodes: sub.snapshot.episodes });
    })
    .catch((e) => setState("status", { state: "failed", reason: reasonOf(e) }));
  return { state, command: (c) => command(c) };
}

export interface DeviceCaps {
  displayHeight: number;
  hdr: string[];
  decoders: string[];
  tenBitHevc: boolean;
  passthrough: string[];
  hasFallbackEngine: boolean;
}

// Codec names as the core's ranking expects them, with a probe for each.
const DECODERS: [name: string, type: string][] = [
  ["h264", 'video/mp4; codecs="avc1.640028"'],
  ["h265", 'video/mp4; codecs="hvc1.1.6.L120.90"'],
  ["av1", 'video/mp4; codecs="av01.0.08M.08"'],
  ["vp9", 'video/webm; codecs="vp9"'],
];

const decodes = (type: string) =>
  (typeof MediaSource !== "undefined" && MediaSource.isTypeSupported(type)) || document.createElement("video").canPlayType(type) === "probably";

/** What this browser can show and decode, so Versions ranks for it. A web
 * player decodes audio itself (no passthrough) and has no second engine. */
export function displayCaps(): DeviceCaps {
  const lines = Math.min(screen.width, screen.height) * (window.devicePixelRatio || 1);
  return {
    displayHeight: lines >= 2000 ? 2160 : lines >= 1000 ? 1080 : 720,
    hdr: matchMedia("(dynamic-range: high)").matches ? ["hdr10", "hlg"] : [],
    decoders: DECODERS.filter(([, type]) => decodes(type)).map(([name]) => name),
    tenBitHevc: decodes('video/mp4; codecs="hvc1.2.4.L120.90"'),
    passthrough: [],
    hasFallbackEngine: false,
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

/** The profile's preferences, kept open for the app's lifetime. */
export function prefsView(): LiveView<Prefs> {
  prefs ??= liveView<Prefs>("settings", {}, true);
  return prefs;
}
