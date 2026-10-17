/** The app's one bridge connection and the live views built on it. Each view
 * is a Solid store fed by the core's snapshot and diffs, so components read
 * plain fields and re-render only what changed. */

import { batch, createSignal, onCleanup } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { Bridge, webSocketTransport } from "./bridge";
import { applyDiffs } from "./diff";
import type { Episode } from "./generated/Episode";
import type { Hero } from "./generated/Hero";
import type { HomeRow } from "./generated/HomeRow";
import type { HomeSnapshot } from "./generated/HomeSnapshot";
import type { HomeUpdate } from "./generated/HomeUpdate";
import type { TitleHeader } from "./generated/TitleHeader";
import type { TitleSnapshot } from "./generated/TitleSnapshot";
import type { TitleUpdate } from "./generated/TitleUpdate";

export const bridgeUrl: string = import.meta.env.VITE_MQ_BRIDGE ?? `ws://${location.hostname || "127.0.0.1"}:7777/ws`;

const transport = webSocketTransport(bridgeUrl);
export const bridge = new Bridge(transport);

export type Link = "connecting" | "open" | "closed";
const [link, setLink] = createSignal<Link>("connecting");
export { link };
transport.onClose(() => setLink("closed"));

type ViewStatus = { state: "opening" } | { state: "open" } | { state: "failed"; reason: string };

const reasonOf = (e: unknown) =>
  e instanceof Error && e.message === "connection closed"
    ? `Cannot reach the core at ${bridgeUrl}. Start it with cargo run -p mq-bridge.`
    : String(e instanceof Error ? e.message : e);

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

/** Installs an addon by manifest URL; resolves to its display name. */
export async function installAddon(url: string): Promise<string> {
  const result = await bridge.request<{ id: string; name: string }>("installAddon", { url });
  setLink("open");
  return result.name;
}
