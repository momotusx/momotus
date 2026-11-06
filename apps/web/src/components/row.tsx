/** A Home row: fixed position, fixed height in every state, and only the
 * tiles near the viewport mounted. Tiles sit at absolute offsets so filling a
 * row never shifts anything and spatial focus sees a stable grid. */

import { A } from "@solidjs/router";
import CircleAlert from "lucide-solid/icons/circle-alert";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Wrench from "lucide-solid/icons/wrench";
import Undo2 from "lucide-solid/icons/undo-2";
import X from "lucide-solid/icons/x";
import { createEffect, createMemo, createRoot, createSignal, For, type JSX, Match, on, onCleanup, onMount, Show, Switch } from "solid-js";
import tokens from "../../../../design/tokens.json";
import { Poster } from "./art";
import { Why } from "./why";
import type { HomeRow } from "../core/generated/HomeRow";
import type { Tile as TileData } from "../core/generated/Tile";
import { op } from "../core/client";
import { onProfileReset } from "../core/profiles";
import { form, scale, viewport } from "../form";
import { pct, posterLabel, titlePath } from "../lib/format";
import { caption } from "../lib/words";
import { wordFit } from "../lib/word-fit";
import { visibleRange } from "../lib/range";
import { rowWords } from "../lib/rows";
import { tileMenu } from "./kids";
import { RouteA } from "./route-link";

export const CONTINUE_ROW = "continue-watching";

interface Metrics {
  poster: [number, number];
  landscape: [number, number];
  gap: number;
  /** Room above and below tiles for the focus scale, ring and halo. */
  pad: number;
  /** Horizontal inset of the first tile inside the scroller. */
  inset: number;
  peek: number;
  caption: number;
}

// From the one token source (design/tokens.json `tile`); `caption` is the room under a
// tile for its two lines (name, then the year or the core's caption).
type TileTokens = Pick<(typeof tokens.tile)["tv"], "poster" | "landscape" | "gap" | "pad" | "inset" | "peek" | "caption">;
const fromTokens = (t: TileTokens): Metrics => ({
  poster: [t.poster.width, t.poster.height],
  landscape: [t.landscape.width, t.landscape.height],
  gap: t.gap,
  pad: t.pad,
  inset: t.inset,
  peek: t.peek,
  caption: t.caption,
});
const TV = fromTokens(tokens.tile.tv);
const PHONE = fromTokens(tokens.tile.phone);
/** Grids (Discover, Library): TV's sizes on desktop until their own phase. */
export const metrics = () => (form() === "phone" ? PHONE : TV);

// Desktop shelves, tv.apple.com's (`tile.desktop`), in CSS pixels on the scaled canvas.
const desktopShelf = (k: number): Metrics => {
  const m = fromTokens(tokens.tile.desktop);
  const px = (n: number) => n / k;
  const size = ([w, h]: [number, number]): [number, number] => [px(w), px(h)];
  return {
    poster: size(m.poster),
    landscape: size(m.landscape),
    gap: px(m.gap),
    pad: px(m.pad),
    inset: px(m.inset),
    peek: px(m.peek),
    caption: px(m.caption),
  };
};
/** Home's shelves per form: TV and phone from their tokens, desktop tv.apple.com's. */
export const shelfMetrics = () => (form() === "desktop" ? desktopShelf(scale()) : metrics());

/** The line under a Top 10 poster off the TV, in canvas pixels. */
const RANK_GENRE = 22;

// Survives the Home screen unmounting, so Back lands on the same offsets;
// another profile's Home starts at the start of each row.
const offsets = new Map<string, number>();
onProfileReset(() => offsets.clear());

export function TileCard(props: {
  tile: TileData;
  fkey: string;
  index?: number;
  w: number;
  h: number;
  autofocus?: boolean;
  style?: Record<string, string>;
  captions?: boolean;
  onOpen?: () => void;
  // Rows mount only tiles near the viewport, so those load at once; native
  // lazy loading would wait until a nested scroller shows them.
  eager?: boolean;
  /** Continue Watching: a 16:9 still with the logo, Apple TV Up Next style. */
  wide?: boolean;
  /** A remove button on the tile (Continue Watching off TV; TV removes from the title page). */
  onRemove?: () => void;
  /** A small label on the poster's corner (Discover's sort value). */
  corner?: JSX.Element | null;
  /** The line under the name in place of the year: null leaves it out (the corner shows the year already). */
  sub?: string | null;
}) {
  // Two lines under the art: the name, then the core's caption (Continue
  // Watching, Library) or the year.
  const cap = () => (props.captions === false ? null : caption(props.tile));
  const sub = () => (props.sub !== undefined ? props.sub : (cap() ?? props.tile.genre ?? props.tile.releaseInfo));
  // A 2:3 poster shows the core's cover (RPDB's, its scores drawn in) and its addon's poster when
  // that does not load; Continue Watching's still crops a poster from its top, so it takes the plain one.
  const plain = () => props.tile.posterFallback ?? props.tile.poster;
  // A button cannot sit inside the link: with a remove button, both go in a
  // slot that takes the tile's place.
  const slotted = !!props.onRemove;
  const card = (
    <RouteA
      href={titlePath(props.tile.kind, props.tile.id)}
      class="tile"
      classList={{ wide: props.wide }}
      data-f={props.fkey}
      data-index={props.index}
      data-autofocus={props.autofocus ? "" : undefined}
      /* The cover's scores are pixels: the label speaks them from the core's ratings. */
      aria-label={posterLabel([props.tile.rank ? `Number ${props.tile.rank}` : null, props.tile.name, cap() ?? props.tile.genre], props.wide ? null : props.tile.ratings)}
      style={slotted ? { width: `${props.w}px` } : { width: `${props.w}px`, ...props.style }}
      onClick={() => props.onOpen?.()}
      {...tileMenu(() => props.tile)}
    >
      <div class="pic" style={{ height: `${props.h}px` }}>
        <Show
          when={props.wide}
          fallback={<Poster src={props.tile.poster} fallback={props.tile.posterFallback} name={props.tile.name} w={props.w} h={props.h} eager={props.eager} />}
        >
          <Poster
            src={props.tile.background ?? plain()}
            fallback={props.tile.background ? plain() : null}
            name={props.tile.name}
            w={props.w}
            h={props.h}
            eager={props.eager}
            nameCard={false}
            top={!props.tile.background}
          />
        </Show>
        {/* A Top 10 tile's place, white over a soft corner shade (plans/appletv-polish.md 4.3). */}
        <Show when={props.tile.rank}>
          {(n) => (
            <span class="rank num" aria-hidden="true">
              {n()}
            </span>
          )}
        </Show>
        <Show when={props.corner}>
          {/* Above the cover's ratings band when it has one (RPDB's covers carry their addon poster as fallback). */}
          <span class="corner" classList={{ banded: !!props.tile.posterFallback }}>
            {props.corner}
          </span>
        </Show>
        {/* A track and a fill on a dark strip, so progress shows on white art. */}
        <Show when={props.tile.progress !== null}>
          <div class="bar">
            <span class="bar-track">
              <i style={{ width: pct(props.tile.progress ?? 0) }} />
            </span>
          </div>
        </Show>
      </div>
      <Show when={props.captions === false && props.tile.genre}>
        {(g) => (
          <p class="rank-genre" aria-hidden="true">
            {g()}
          </p>
        )}
      </Show>
      <Show when={props.captions !== false}>
        {/* The name, then the year or the core's caption: no scores (the cover carries them). */}
        <p class="cap">
          <b ref={(el) => fitWords(el, () => props.tile.name)} />
          <Show when={sub()}>
            <span class="num" ref={(el) => fitWords(el, () => sub() ?? "")} />
          </Show>
        </p>
      </Show>
    </RouteA>
  );
  if (!slotted) return card;
  return (
    <div class="tile-slot" style={{ width: `${props.w}px`, ...props.style }}>
      {card}
      <button
        class="tile-x"
        data-index={props.index}
        aria-label={`Remove ${props.tile.name} from Continue Watching`}
        title="Remove from Continue Watching"
        onClick={() => props.onRemove?.()}
      >
        <X aria-hidden />
      </button>
    </div>
  );
}

// The text face draws wider than its fallback: lines measured before a face loaded are measured again.
const [fonts, setFonts] = createSignal(0);
document.fonts?.addEventListener("loadingdone", () => setFonts((n) => n + 1));
// Captions change with the window's width only: the phone's URL bar showing or hiding mid-fling sets a new height.
const windowWidth = createRoot(() => createMemo(() => viewport().w));

// Every caption waiting for its frame. One frame reads every line's width and style first, then measures the words
// on a canvas and writes the lines: one layout for all of them, none per word or per line.
const fitting = new Map<HTMLElement, string>();
let fitFrame = 0;
let pen: CanvasRenderingContext2D | null | undefined;

function fitAll() {
  fitFrame = 0;
  const lines = [...fitting].map(([el, text]) => {
    const s = getComputedStyle(el);
    return { el, text, w: el.clientWidth, font: `${s.fontStyle} ${s.fontWeight} ${s.fontSize} ${s.fontFamily}`, spacing: parseFloat(s.letterSpacing) || 0 };
  });
  fitting.clear();
  pen ??= document.createElement("canvas").getContext("2d");
  for (const { el, text, w, font, spacing } of lines) {
    // A line not laid out (its row folded, a hidden page) keeps its words until it is measured again.
    if (!pen || w === 0) continue;
    pen.font = font;
    // CSS spaces every character, the last one too.
    el.textContent = wordFit(text, (line) => pen!.measureText(line).width + spacing * [...line].length <= w);
  }
}

/** Keeps a caption line (`nowrap`, `overflow: hidden`) at whole words of `text`, again when the window changes. */
function fitWords(el: HTMLElement, text: () => string) {
  let last: string | null = null;
  createEffect(() => {
    const t = text();
    void windowWidth();
    void fonts();
    // New words show at once; a resize keeps the fitted line until it is measured again (no flash of the cut one).
    if (t !== last) el.textContent = t;
    last = t;
    fitting.set(el, t);
    fitFrame ||= requestAnimationFrame(fitAll);
    onCleanup(() => fitting.delete(el));
  });
}

/** "Remove from Continue Watching" with five seconds for Undo, as Android's PendingRemoval: Home leaves the title
 * out at once, and it goes to the core when the time passes, at once when another title is removed or the page goes
 * (a synced register, so a slip would hide it on every device). A failure is said and the title comes back. The X on
 * a tile and TV's button on the title page. */
const UNDO_MS = 5000;
const [removal, setRemoval] = createSignal<{ id: string; name: string } | null>(null);
// Sent and not yet gone from the core's own row: still left out, so the tile never flashes back.
const [sent, setSent] = createSignal<string[]>([]);
const [removeFailed, setRemoveFailed] = createSignal<string | null>(null);
let waiting: ReturnType<typeof setTimeout> | undefined;

/** Whether Continue Watching leaves this title out: waiting for its Undo, or sent. */
export const leftOut = (id: string) => removal()?.id === id || sent().includes(id);

/** The core's row no longer has these titles: nothing to leave out for them any more. */
export function settleRemovals(ids: readonly string[]) {
  if (sent().some((id) => !ids.includes(id))) setSent((s) => s.filter((id) => ids.includes(id)));
}

function send(r: { id: string; name: string }) {
  if (removal()?.id === r.id) setRemoval(null);
  setSent((s) => [...s, r.id]);
  removeFromContinue(r.id).catch(() => {
    setSent((s) => s.filter((id) => id !== r.id));
    const words = `${r.name} could not be removed from Continue Watching.`;
    setRemoveFailed(words);
    setTimeout(() => removeFailed() === words && setRemoveFailed(null), UNDO_MS * 2);
  });
}

export function removeSoon(tile: { id: string; name: string }) {
  clearTimeout(waiting);
  const earlier = removal();
  if (earlier) send(earlier);
  setRemoveFailed(null);
  setRemoval(tile);
  waiting = setTimeout(() => send(tile), UNDO_MS);
  // The pressed control leaves with its tile: Undo takes the focus.
  requestAnimationFrame(() => document.querySelector<HTMLElement>('[data-f="continue:undo"]')?.focus());
}

/** Undo: nothing goes to the core, and the title is back, its control with the focus. */
function undoRemoval() {
  const r = removal();
  dropRemoval();
  if (r) requestAnimationFrame(() => document.querySelector<HTMLElement>(`[data-f="title:uncontinue"], [data-f$=":continue-watching:${CSS.escape(r.id)}"]`)?.focus());
}

/** A title that plays is wanted: its removal waiting for Undo is dropped, never sent while it plays. */
export function played(id: string) {
  if (removal()?.id === id) dropRemoval();
}

function dropRemoval() {
  clearTimeout(waiting);
  setRemoval(null);
}

// A new profile: a removal still waiting is dropped, never sent for the next profile.
onProfileReset(() => {
  dropRemoval();
  setSent([]);
  setRemoveFailed(null);
});

// The page going (or hidden, which may be the last chance a phone gives it): a removal still waiting goes now.
const sendNow = () => {
  const r = removal();
  if (!r) return;
  clearTimeout(waiting);
  send(r);
};
window.addEventListener("pagehide", sendNow);
document.addEventListener("visibilitychange", () => document.hidden && sendNow());

/** After Remove: the words with Undo while it can be taken back, or why it failed. */
export function RemovalNote() {
  return (
    <div class="removal-note" role="status">
      <Show when={removal()} fallback={removeFailed()}>
        {(r) => (
          <>
            <span>{r().name} was removed from Continue Watching.</span>
            <button class="btn" data-f="continue:undo" onClick={undoRemoval}>
              <Undo2 aria-hidden />
              Undo
            </button>
          </>
        )}
      </Show>
    </div>
  );
}

/** "Remove from Continue Watching": the core drops the title from Continue
 * Watching and Next Up on every synced device until it is played again, and
 * Home's row updates from the core. */
export const removeFromContinue = (itemId: string) => op("removeFromContinue", { itemId });

export function Row(props: {
  row: HomeRow;
  root: () => Element | null;
  onOpen?: () => void;
  scope?: string;
  onRetry?: (rowId: string) => Promise<unknown> | undefined;
}) {
  let section!: HTMLElement;
  let track: HTMLDivElement | undefined;
  const m = shelfMetrics;
  // Search shows the same catalogs as Home: keep their offsets and keys apart.
  const id = () => `${props.scope ?? "home"}:${props.row.rowId}`;
  // Continue Watching is a row of 16:9 stills with progress; every catalog row is 2:3 posters on every
  // form, as Stremio's, each the core's RPDB cover with its ratings band (the owner, 2026-10-23).
  const wide = () => props.row.rowId === CONTINUE_ROW;
  const size = () => (wide() ? m().landscape : m().poster);
  const stride = () => size()[0] + m().gap;
  // Captions under the art: every TV tile; elsewhere Continue Watching alone (posters show none).
  const captions = () => form() === "tv" || wide();
  // A Top 10 row off the TV gives its genre one line under the art.
  const captionHeight = () => (captions() ? m().caption : props.row.ranked ? RANK_GENRE / (form() === "desktop" ? scale() : 1) : 0);
  const bodyHeight = () => size()[1] + m().pad * 2 + captionHeight();
  const [scroll, setScroll] = createSignal(offsets.get(id()) ?? 0);
  const [viewport, setViewport] = createSignal(1920);
  const [focused, setFocused] = createSignal(-1);
  const [near, setNear] = createSignal(true);
  const range = createMemo(
    () => (near() ? visibleRange(scroll() - m().inset, viewport(), stride(), props.row.tiles.length, focused()) : ([0, 0] as const)),
    [0, 0] as const,
    { equals: (a, b) => a[0] === b[0] && a[1] === b[1] },
  );
  const indices = createMemo(() => {
    const [a, b] = range();
    return Array.from({ length: b - a }, (_, i) => a + i);
  });

  onMount(() => {
    const measure = () => track && setViewport(track.clientWidth);
    measure();
    window.addEventListener("resize", measure, { passive: true });
    const io = new IntersectionObserver(([e]) => e && setNear(e.isIntersecting), { root: props.root(), rootMargin: "100% 0px" });
    io.observe(section);
    onCleanup(() => {
      window.removeEventListener("resize", measure);
      io.disconnect();
    });
  });

  const attachTrack = (el: HTMLDivElement) => {
    track = el;
    const saved = offsets.get(id());
    if (saved) queueMicrotask(() => el.scrollTo({ left: saved }));
  };

  const ready = () => props.row.tiles.length > 0;
  const state = () => props.row.state.state;
  // A Try again under way: until the row answers or fails again.
  const [retrying, setRetrying] = createSignal(false);
  createEffect(on(state, (s) => s !== "loading" && s !== "slow" && setRetrying(false), { defer: true }));

  return (
    <section class="row" data-row data-fgroup={`row:${id()}`} ref={section} aria-label={props.row.title}>
      <header class="row-head">
        <h2>{props.row.title}</h2>
        <Show when={state() === "slow" && !ready()}>
          <span class="st warn row-note">
            <i class="dot" />
            {rowWords.slow}
          </span>
        </Show>
      </header>
      <div class="row-body" style={{ height: `${bodyHeight()}px` }}>
        <Switch>
          {/* While a Try again runs, the message and its button stay (the
              button reads "Trying again"), so the remote keeps its place; the
              tiles that come take the focus from it, a new failure gives it
              back its words. */}
          <Match when={(props.row.state.state === "failed" || retrying()) && !ready()}>
            <div class="row-msg">
              <Show when={retrying()} fallback={<CircleAlert aria-hidden class="row-msg-icon bad-ico" />}>
                <LoaderCircle aria-hidden class="row-msg-icon spin" />
              </Show>
              <div>
                <p>
                  {retrying() ? rowWords.retrying : rowWords.failed}
                </p>
                <Show when={!retrying() && props.row.state.state === "failed"}>
                  <Why class="why" reason={props.row.state.state === "failed" ? props.row.state.reason : ""} />
                </Show>
              </div>
              <div class="row-acts">
                <Show when={props.onRetry}>
                  {(retry) => (
                    <button
                      class="btn"
                      data-f={`retry:${id()}`}
                      aria-disabled={retrying()}
                      onClick={() => {
                        if (retrying()) return;
                        setRetrying(true);
                        Promise.resolve(retry()(props.row.rowId)).catch(() => setRetrying(false));
                      }}
                    >
                      <RefreshCw aria-hidden />
                      {retrying() ? "Trying again" : "Try again"}
                    </button>
                  )}
                </Show>
                <A class="btn" href="/welcome?step=3" data-f={`fix:${id()}`}>
                  <Wrench aria-hidden />
                  Run setup check
                </A>
              </div>
            </div>
          </Match>
          <Match when={state() === "empty"}>
            <div class="row-msg">
              <p>{rowWords.empty}</p>
            </div>
          </Match>
          <Match when={!ready()}>
            <div class="lane skel-lane" aria-hidden="true">
              <For each={Array.from({ length: 8 })}>
                {(_, i) => (
                  <div
                    class="skel pic"
                    style={{ left: `${m().inset + i() * stride()}px`, top: `${m().pad}px`, width: `${size()[0]}px`, height: `${size()[1]}px` }}
                  />
                )}
              </For>
            </div>
          </Match>
          <Match when={ready()}>
            {/* Never a stop of its own: a row far from view draws no tiles, and Chrome makes a scroller with
                nothing focusable inside a Tab stop. Its tiles carry the focus. */}
            <div
              class="track"
              tabIndex={-1}
              data-hscroll={m().inset + m().peek}
              ref={attachTrack}
              onScroll={(e) => {
                const left = e.currentTarget.scrollLeft;
                offsets.set(id(), left);
                setScroll(left);
              }}
              onFocusIn={(e) => setFocused(Number((e.target as HTMLElement).dataset.index ?? -1))}
            >
              <div class="lane" style={{ width: `${m().inset * 2 + props.row.tiles.length * stride() - m().gap}px` }}>
                <For each={indices()}>
                  {(i) => (
                    <Show when={props.row.tiles[i]}>
                      {(tile) => (
                        <TileCard
                          tile={tile()}
                          fkey={`tile:${id()}:${tile().id}`}
                          index={i}
                          style={{ position: "absolute", left: `${m().inset + i * stride()}px`, top: `${m().pad}px` }}
                          w={size()[0]}
                          h={size()[1]}
                          onOpen={props.onOpen}
                          eager
                          wide={wide()}
                          captions={captions()}
                          onRemove={wide() && form() !== "tv" ? () => removeSoon(tile()) : undefined}
                        />
                      )}
                    </Show>
                  )}
                </For>
              </div>
            </div>
          </Match>
        </Switch>
      </div>
    </section>
  );
}
