/** The download control (plans/offline-downloads.md 3.1): its glyph, in a determinate ring while a
 * download is under way, and the menu a press opens, plus the ops behind them. The words and the
 * menus come from lib/download.ts. Nothing of it shows on TV or on a kids profile (`downloadsHere`). */

import ArrowDownToLine from "lucide-solid/icons/arrow-down-to-line";
import CircleAlert from "lucide-solid/icons/circle-alert";
import CircleCheck from "lucide-solid/icons/circle-check";
import Clock from "lucide-solid/icons/clock";
import Layers from "lucide-solid/icons/layers";
import Pause from "lucide-solid/icons/pause";
import Play from "lucide-solid/icons/play";
import RotateCw from "lucide-solid/icons/rotate-cw";
import Smartphone from "lucide-solid/icons/smartphone";
import Square from "lucide-solid/icons/square";
import Trash from "lucide-solid/icons/trash";
import WifiOff from "lucide-solid/icons/wifi-off";
import X from "lucide-solid/icons/x";
import { A, useLocation, useNavigate } from "@solidjs/router";
import { type Component, createEffect, For, type JSX, onCleanup, onMount, Show } from "solid-js";
import { Dynamic } from "solid-js/web";
import { displayCaps, liveView, online, op } from "../core/client";
import type { DownloadBadge } from "../core/generated/DownloadBadge";
import type { DownloadsSnapshot } from "../core/generated/DownloadsSnapshot";
import { profiles } from "../core/profiles";
import { pushBack } from "../focus/back";
import { focusEl } from "../focus/dom";
import { form } from "../form";
import { controlName, type DownloadControl, type DownloadGlyph, type DownloadItem, downloadWords, ITEM_LABEL, itemOp, kidsOf, offlineBanner, shownDownloads } from "../lib/download";
import { downloadProblem, downloadWordsFailed, reported } from "../lib/download-status";
import "./download.css";

type Icon = Component<{ "aria-hidden"?: boolean; class?: string; style?: JSX.CSSProperties }>;

const GLYPH: Record<DownloadGlyph, Icon> = {
  download: ArrowDownToLine,
  stop: Square,
  pause: Pause,
  wifiOff: WifiOff,
  clock: Clock,
  check: CircleCheck,
  alert: CircleAlert,
};

export const ITEM_ICON: Record<DownloadItem, Icon> = {
  start: ArrowDownToLine,
  again: ArrowDownToLine,
  choose: Layers,
  another: Layers,
  pause: Pause,
  resume: Play,
  play: Play,
  cancel: X,
  delete: Trash,
  remove: Trash,
  retry: RotateCw,
  mobileData: Smartphone,
};

/** Downloads show on the phone and desktop forms, never on TV, and a kids profile gets only Play. */
export const downloadsHere = () => form() !== "tv";
/** Downloads' own pages are not for TV, even by address: they give way to Home. */
export function awayFromTv() {
  const navigate = useNavigate();
  createEffect(() => {
    if (form() === "tv") navigate("/", { replace: true });
  });
}

/** Unknown means kids: no download control until the active profile is known not to be one. */
export const kidsProfile = () => kidsOf(profiles());
/** Known to be a kids profile: its kept titles read "Downloaded". */
export const knownKids = () => profiles()?.profiles.find((p) => p.id === profiles()?.active)?.kids === true;
/** A touch screen has no secondary press everywhere (iOS Safari fires no contextmenu): a tap on Download
 * opens its menu there, with Choose a version to download beside Download. */
export const touchFirst = () => typeof matchMedia === "function" && matchMedia("(pointer: coarse)").matches;

/** The glyph, in a conic ring at its value while a download is under way (white on the 10 percent track). */
export function DownloadMark(props: { control: DownloadControl }) {
  return (
    <span class="dl-mark" classList={{ ring: props.control.ring !== null }} style={{ "--p": `${(props.control.ring ?? 0) * 100}%` }} aria-hidden="true">
      <Dynamic
        component={GLYPH[props.control.glyph]}
        aria-hidden
        class={props.control.glyph === "stop" || props.control.glyph === "pause" ? "fill" : undefined}
        style={{ width: "var(--dl-g)", height: "var(--dl-g)", padding: 0, background: "none", "box-shadow": "none", transition: "none" }}
      />
    </span>
  );
}

/** The accessible side of a control: a name that starts with what it shows, then its state, value and
 * subject (lib/download.ts controlName), and that a press opens a menu. */
export const controlAria = (c: DownloadControl, subject: string) => ({
  "aria-label": controlName(c, subject),
  "aria-haspopup": c.hint ? ("menu" as const) : undefined,
});

/** The one line a refused op leaves (lib/download-status.ts), over whatever page is open. */
export function DownloadStatus() {
  return (
    <Show when={downloadProblem()}>
      {(words) => (
        <p class="dl-status st bad" role="status">
          <i class="dot" />
          {words()}
        </p>
      )}
    </Show>
  );
}

/** The shell's offline banner (plans/offline-downloads.md 3.5): above every page but TV's and the
 * player's while the browser is offline, in its own height (the shell's pages start under it), said
 * once when it shows. The downloads view opens only while offline, to know whether any of this
 * profile's downloads is done. */
export function OfflineBanner() {
  return (
    <Show when={!online() && form() !== "tv"}>
      <OfflineBar />
    </Show>
  );
}

function OfflineBar() {
  const location = useLocation();
  const view = liveView<DownloadsSnapshot>("downloads");
  const hasDone = () => !!shownDownloads(view.state.data)?.titles.some((t) => t.rows.some((r) => r.state === "done"));
  const banner = () => offlineBanner(online(), form(), location.pathname, hasDone());
  // The pages below start under the banner, whatever it wraps to (`--offline-h` on the shell).
  const measure = (el: HTMLElement) => {
    const shell = el.parentElement;
    if (!shell) return;
    const set = () => shell.style.setProperty("--offline-h", `${el.offsetHeight}px`);
    const watch = new ResizeObserver(set);
    watch.observe(el);
    onCleanup(() => {
      watch.disconnect();
      shell.style.removeProperty("--offline-h");
    });
  };
  return (
    <Show when={banner()}>
      {(b) => (
        <section class="offline-bar" role="status" ref={measure} data-fgroup="offline-banner">
          <WifiOff aria-hidden />
          <p>{b().words}</p>
          <Show when={b().action}>
            <A class="btn" href="/library/downloads" data-f="offline:downloads">
              <ArrowDownToLine aria-hidden />
              {downloadWords.offlineAction}
            </A>
          </Show>
        </section>
      )}
    </Show>
  );
}

/** Downloads a video, a season's aired episodes or every regular season's; `pick` is a row of an open
 * download-purpose Versions view. */
export function downloadAdd(kind: string, itemId: string, scope: "video" | "season" | "series", extra: { videoId?: string; season?: number; pick?: { versionsSub: number; key: string } } = {}) {
  void reported(op("downloadAdd", { kind, itemId, scope, params: displayCaps(), ...extra }), downloadWordsFailed.add);
}

export type Which = { ids: string[] } | { itemId: string } | { all: true };

export function downloadAct(name: string, which: Which, mobileData = false) {
  void reported(op(name, { ...which, ...(mobileData ? { mobileData } : {}) }), downloadWordsFailed.change);
}

/** An item of an existing download's menu that is an op: sent, true. The rest go somewhere and are the caller's. */
export function runItem(item: DownloadItem, badge: DownloadBadge | null): boolean {
  const o = itemOp(item);
  if (!o) return false;
  if (badge) downloadAct(o.op, { ids: [badge.id] }, o.mobileData);
  return true;
}

export interface MenuEntry {
  key: string;
  label: string;
  icon?: Icon;
  /** Null: a line that says where things stand; pressing it does nothing. */
  run: (() => void) | null;
}

/** A menu to open: titled `title`, the reason in full under it, its entries; `from` is the
 * `data-f` of the control that opened it, which takes the focus back. */
export interface MenuAsk {
  title: string;
  detail?: string | null;
  entries: MenuEntry[];
  from: string;
  /** The entry the focus starts on (a confirmation's Cancel), else the first. */
  focus?: string;
}

/** A download's items as menu entries; `go` runs the ones that lead somewhere (Download, a version, Play). */
export const itemEntries = (items: DownloadItem[], badge: DownloadBadge | null, go: (item: DownloadItem) => void): MenuEntry[] =>
  items.map((item) => ({ key: item, label: ITEM_LABEL[item], icon: ITEM_ICON[item], run: () => runItem(item, badge) || go(item) }));

/** The menu over the page, as the synopsis card: modal, Back or a press outside closes it, and the
 * focus returns to the control. Up and Down move through its items. */
export function DownloadMenu(props: { ask: MenuAsk; onClose: () => void }) {
  let list!: HTMLDivElement;
  const from = props.ask.from;
  const close = () => {
    props.onClose();
    queueMicrotask(() => {
      const opener = document.querySelector<HTMLElement>(`[data-f="${CSS.escape(from)}"]`);
      if (opener) focusEl(opener);
    });
  };
  onMount(() => {
    onCleanup(pushBack(close));
    queueMicrotask(() => {
      const want = props.ask.focus;
      const first = (want && list.querySelector<HTMLElement>(`[data-f="dlmenu:${CSS.escape(want)}"]`)) || list.querySelector<HTMLElement>("[data-f]");
      if (first) focusEl(first);
    });
  });
  const keys = (e: KeyboardEvent) => {
    const items = [...list.querySelectorAll<HTMLElement>("[data-f]")];
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = items[Math.min(items.length - 1, Math.max(0, at + (e.key === "ArrowDown" ? 1 : -1)))];
      if (next) focusEl(next);
    } else if (e.key === "ArrowLeft" || e.key === "ArrowRight" || e.key === "Tab") e.preventDefault();
  };
  return (
    <div class="dl-layer" role="dialog" aria-modal="true" aria-labelledby="dl-menu-title">
      <div class="dl-scrim" aria-hidden="true" onClick={close} />
      <section class="dl-card" data-fgroup="dl-menu">
        <h2 id="dl-menu-title">{props.ask.title}</h2>
        <Show when={props.ask.detail}>{(d) => <p class="dl-detail">{d()}</p>}</Show>
        <div class="dl-items" role="menu" aria-labelledby="dl-menu-title" ref={list} onKeyDown={keys}>
          <For each={props.ask.entries}>
            {(entry) => (
              <Show
                when={entry.run}
                fallback={
                  <p class="dl-item still" role="none">
                    {entry.label}
                  </p>
                }
              >
                {(run) => (
                  <button
                    class="dl-item"
                    role="menuitem"
                    data-f={`dlmenu:${entry.key}`}
                    onClick={() => {
                      close();
                      run()();
                    }}
                  >
                    <Show when={entry.icon}>{(icon) => <Dynamic component={icon()} aria-hidden />}</Show>
                    {entry.label}
                  </button>
                )}
              </Show>
            )}
          </For>
        </div>
      </section>
    </div>
  );
}
