/** A Home row: fixed position, fixed height in every state, and only the
 * tiles near the viewport mounted. Tiles sit at absolute offsets so filling a
 * row never shifts anything and spatial focus sees a stable grid. */

import { A } from "@solidjs/router";
import CircleAlert from "lucide-solid/icons/circle-alert";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Wrench from "lucide-solid/icons/wrench";
import X from "lucide-solid/icons/x";
import { createEffect, createMemo, createSignal, For, Match, on, onCleanup, onMount, Show, Switch } from "solid-js";
import tokens from "../../../../design/tokens.json";
import { Poster } from "./art";
import { Ratings } from "./ratings";
import { Why } from "./why";
import type { HomeRow } from "../core/generated/HomeRow";
import type { Tile as TileData } from "../core/generated/Tile";
import { op } from "../core/client";
import { onProfileReset } from "../core/profiles";
import { form, scale } from "../form";
import { pct, posterLabel, ratingParts, titlePath } from "../lib/format";
import { caption } from "../lib/words";
import { visibleRange } from "../lib/range";
import { rowWords } from "../lib/rows";

export const CONTINUE_ROW = "continue-watching";

interface Metrics {
  poster: [number, number];
  landscape: [number, number];
  square: [number, number];
  gap: number;
  /** Room above and below tiles for the focus scale, ring and halo. */
  pad: number;
  /** Horizontal inset of the first tile inside the scroller. */
  inset: number;
  peek: number;
  caption: number;
  /** A 2:3 poster's caption: on TV the name, then the year and the ratings row on two lines. */
  captionPoster: number;
}

// From the one token source (design/tokens.json `tile`); `caption` is the room under a
// tile for its two lines (name, then the year or the core's caption).
type TileTokens = Pick<(typeof tokens.tile)["tv"], "poster" | "landscape" | "square" | "gap" | "pad" | "inset" | "peek" | "caption">;
const fromTokens = (t: TileTokens): Metrics => ({
  poster: [t.poster.width, t.poster.height],
  landscape: [t.landscape.width, t.landscape.height],
  square: [t.square.width, t.square.height],
  gap: t.gap,
  pad: t.pad,
  inset: t.inset,
  peek: t.peek,
  caption: t.caption,
  captionPoster: t.caption,
});
const TV = { ...fromTokens(tokens.tile.tv), captionPoster: tokens.tile.tv.captionPoster };
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
    square: size(m.square),
    gap: px(m.gap),
    pad: px(m.pad),
    inset: px(m.inset),
    peek: px(m.peek),
    caption: px(m.caption),
    captionPoster: px(m.caption),
  };
};
/** Home's shelves per form: TV and phone from their tokens, desktop tv.apple.com's. */
export const shelfMetrics = () => (form() === "desktop" ? desktopShelf(scale()) : metrics());

const sizeOf = (m: Metrics, shape: string | undefined) =>
  shape === "landscape" ? m.landscape : shape === "square" ? m.square : m.poster;

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
  /** A 16:9 still (a TV movie row): the backdrop, else the poster from its upper part. */
  still?: boolean;
  /** A remove button on the tile (Continue Watching off TV; TV removes from the title page). */
  onRemove?: () => Promise<unknown>;
  /** A TV shelf's caption: the year then the ratings row (one line under a still, two under a
   * poster), so the art carries no score chip. */
  rated?: boolean;
}) {
  // Two lines under the art: the name, then the core's caption (Continue
  // Watching, Library) or the year.
  const cap = () => (props.captions === false ? null : caption(props.tile));
  const sub = () => cap() ?? props.tile.releaseInfo;
  // A button cannot sit inside the link: with a remove button, both go in a
  // slot that takes the tile's place.
  const slotted = !!props.onRemove;
  const card = (
    <A
      href={titlePath(props.tile.kind, props.tile.id)}
      class="tile"
      classList={{ wide: props.wide }}
      data-f={props.fkey}
      data-index={props.index}
      data-autofocus={props.autofocus ? "" : undefined}
      /* The chip is hidden from screen readers: the label speaks its scores. */
      aria-label={posterLabel([props.tile.name, cap()], props.wide ? null : props.tile.ratings)}
      style={slotted ? { width: `${props.w}px` } : { width: `${props.w}px`, ...props.style }}
      onClick={() => props.onOpen?.()}
    >
      <div class="pic" style={{ height: `${props.h}px` }}>
        <Show
          when={props.wide || props.still}
          fallback={<Poster src={props.tile.poster} name={props.tile.name} w={props.w} h={props.h} eager={props.eager} />}
        >
          <Poster
            src={props.tile.background ?? props.tile.poster}
            fallback={props.tile.background ? props.tile.poster : null}
            name={props.tile.name}
            w={props.w}
            h={props.h}
            eager={props.eager}
            nameCard={!props.wide}
            top={!props.tile.background}
          />
        </Show>
        {/* The score chip rides on a grid's poster. Home's shelves draw none (the frames): on TV the
            scores are the caption's ratings row, off the TV a shelf poster has no caption, and the
            label speaks them either way. */}
        <Show when={!props.wide && !props.rated && props.captions !== false && ratingParts(props.tile.ratings).length}>
          <span class="chip-rate num" aria-hidden="true">
            <For each={ratingParts(props.tile.ratings)}>{(p) => <span>{p.text}</span>}</For>
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
      <Show when={props.captions !== false}>
        <p class="cap" classList={{ rated: props.rated, two: props.rated && !props.still }}>
          <b>{props.tile.name}</b>
          <Show
            when={props.rated}
            fallback={
              <Show when={sub()}>
                <span class="num">{sub()}</span>
              </Show>
            }
          >
            <span class="cap-line">
              <span class="num">{sub()}</span>
              <span class="cap-rates">
                <Ratings ratings={props.tile.ratings} sep={() => " "} />
              </span>
            </span>
          </Show>
        </p>
      </Show>
    </A>
  );
  if (!slotted) return card;
  const { ask, press } = askOnce(() => props.onRemove?.() ?? Promise.resolve());
  return (
    <div class="tile-slot" classList={{ failed: ask() === "failed" }} style={{ width: `${props.w}px`, ...props.style }}>
      {card}
      <button
        class="tile-x"
        classList={{ asking: ask() === "confirm", failed: ask() === "failed" }}
        data-index={props.index}
        aria-label={ask() === "confirm" ? `Press again to remove ${props.tile.name} from Continue Watching` : `Remove ${props.tile.name} from Continue Watching`}
        title="Remove from Continue Watching"
        onClick={press}
      >
        <Switch fallback={<X aria-hidden />}>
          <Match when={ask() === "confirm"}>Press again to remove</Match>
          <Match when={ask() === "failed"}>Not removed, try again</Match>
        </Switch>
      </button>
    </div>
  );
}

/** Removing from Continue Watching reaches every synced device, so it asks
 * once ("Press again to remove", 4 s, as Remove on an addon) and says when
 * the core refused: the X on a tile and TV's button on the title page. */
export function askOnce(act: () => Promise<unknown>) {
  const [ask, setAsk] = createSignal<"idle" | "confirm" | "failed">("idle");
  let timer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(timer));
  const settle = (state: "confirm" | "failed") => {
    setAsk(state);
    clearTimeout(timer);
    timer = setTimeout(() => setAsk("idle"), 4000);
  };
  const press = () => {
    if (ask() !== "confirm") return settle("confirm");
    clearTimeout(timer);
    setAsk("idle");
    act().catch(() => settle("failed"));
  };
  return { ask, press };
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
  // Continue Watching is a row of 16:9 stills; on TV a movie row is too and a series row is
  // 2:3 posters (design/DECISION.md, Shelves); other catalogs keep the shape their addon names.
  const wide = () => props.row.rowId === CONTINUE_ROW;
  // A row still loading has no tile to tell its kind: its id names it (`<addonId>@<install>:<kind>:<catalogId>`,
  // views/home.rs row_id), so its skeleton already has the final height and nothing below jumps when tiles
  // arrive. The kind is the part after the one carrying the install tag, so a catalog id with colons is fine.
  // ponytail: parsed from the id; a `kind` field on HomeRow is the sturdy source, but it touches 13 core and
  // 26 Android construction sites, so it waits for the next HomeRow change. A wrong guess only brings back
  // the jump: the loaded row reads its tiles' kind.
  const kind = () => props.row.tiles[0]?.kind ?? props.row.rowId.split(":").find((_, i, parts) => parts[i - 1]?.includes("@"));
  const still = () => !wide() && form() === "tv" && kind() === "movie";
  const size = () => (wide() || still() ? m().landscape : sizeOf(m(), props.row.tiles[0]?.posterShape));
  const stride = () => size()[0] + m().gap;
  // Captions under the art: every TV tile; elsewhere Continue Watching alone (posters show none).
  const captions = () => form() === "tv" || wide();
  // On TV a catalog's caption carries the ratings row: one line under a still, two under a poster.
  const rated = () => form() === "tv" && !wide();
  const captionHeight = () => (!captions() ? 0 : rated() && !still() ? m().captionPoster : m().caption);
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
            <div
              class="track"
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
                          still={still()}
                          captions={captions()}
                          rated={rated()}
                          onRemove={wide() && form() !== "tv" ? () => removeFromContinue(tile().id) : undefined}
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
