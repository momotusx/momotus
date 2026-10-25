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
import Trash2 from "lucide-solid/icons/trash";
import WifiOff from "lucide-solid/icons/wifi-off";
import X from "lucide-solid/icons/x";
import { type Component, For, type JSX, onCleanup, onMount, Show } from "solid-js";
import { Dynamic } from "solid-js/web";
import { displayCaps, op } from "../core/client";
import type { DownloadBadge } from "../core/generated/DownloadBadge";
import { profiles } from "../core/profiles";
import { pushBack } from "../focus/back";
import { focusEl } from "../focus/dom";
import { form } from "../form";
import { type DownloadControl, type DownloadGlyph, type DownloadItem, ITEM_LABEL, itemOp } from "../lib/download";
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
  delete: Trash2,
  remove: Trash2,
  retry: RotateCw,
  mobileData: Smartphone,
};

/** Downloads show on the phone and desktop forms, never on TV, and a kids profile gets only Play. */
export const downloadsHere = () => form() !== "tv";
export const kidsProfile = () => profiles()?.profiles.find((p) => p.id === profiles()?.active)?.kids ?? false;

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

/** The accessible side of a control: its state's word as its name, the ring's value, and that a press
 * opens a menu. */
export const controlAria = (c: DownloadControl) => ({
  "aria-label": c.spoken,
  "aria-valuetext": c.value ?? undefined,
  "aria-haspopup": c.hint ? ("menu" as const) : undefined,
});

// Ops (plans/core-protocol.md, Ops): each answers at once and the views follow.
const quietly = (p: Promise<unknown>) => void p.catch((e: unknown) => console.warn("download op failed", e));

/** Downloads a video, a season's aired episodes or every regular season's; `pick` is a row of an open
 * download-purpose Versions view. */
export function downloadAdd(kind: string, itemId: string, scope: "video" | "season" | "series", extra: { videoId?: string; season?: number; pick?: { versionsSub: number; key: string } } = {}) {
  quietly(op("downloadAdd", { kind, itemId, scope, params: displayCaps(), ...extra }));
}

export type Which = { ids: string[] } | { itemId: string } | { all: true };

export function downloadAct(name: string, which: Which, mobileData = false) {
  quietly(op(name, { ...which, ...(mobileData ? { mobileData } : {}) }));
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
      const first = list.querySelector<HTMLElement>("[data-f]");
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
