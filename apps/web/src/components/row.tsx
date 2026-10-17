/** A Home row: fixed position, fixed height in every state, and only the
 * tiles near the viewport mounted. Tiles sit at absolute offsets so filling a
 * row never shifts anything and spatial focus sees a stable grid. */

import { A } from "@solidjs/router";
import CircleAlert from "lucide-solid/icons/circle-alert";
import Wrench from "lucide-solid/icons/wrench";
import { createMemo, createSignal, For, Match, onCleanup, onMount, Show, Switch } from "solid-js";
import { Poster, useTrimmedLogo } from "./art";
import type { HomeRow } from "../core/generated/HomeRow";
import type { Tile as TileData } from "../core/generated/Tile";
import { form } from "../form";
import { pct, titlePath } from "../lib/format";
import { caption } from "../lib/words";
import { visibleRange } from "../lib/range";

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
}

const TV: Metrics = { poster: [240, 360], landscape: [384, 216], square: [300, 300], gap: 32, pad: 34, inset: 34, peek: 140, caption: 52 };
const PHONE: Metrics = { poster: [112, 168], landscape: [240, 135], square: [140, 140], gap: 12, pad: 8, inset: 16, peek: 40, caption: 40 };
export const metrics = () => (form() === "phone" ? PHONE : TV);

const sizeOf = (m: Metrics, shape: string | undefined) =>
  shape === "landscape" ? m.landscape : shape === "square" ? m.square : m.poster;

// Survives the Home screen unmounting, so Back lands on the same offsets.
const offsets = new Map<string, number>();

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
}) {
  const cap = () => (props.captions === false ? null : caption(props.tile));
  const [logoBroken, setLogoBroken] = createSignal(false);
  const tileLogo = useTrimmedLogo(() => (props.wide ? props.tile.logo : null));
  return (
    <A
      href={titlePath(props.tile.kind, props.tile.id)}
      class="tile"
      classList={{ wide: props.wide }}
      data-f={props.fkey}
      data-index={props.index}
      data-autofocus={props.autofocus ? "" : undefined}
      aria-label={cap() ? `${props.tile.name}, ${cap()}` : props.tile.name}
      style={{ width: `${props.w}px`, ...props.style }}
      onClick={() => props.onOpen?.()}
    >
      <div class="pic" style={{ height: `${props.h}px` }}>
        <Show
          when={props.wide}
          fallback={<Poster src={props.tile.poster} name={props.tile.name} w={props.w} h={props.h} eager={props.eager} />}
        >
          <Poster
            src={props.tile.background ?? props.tile.poster}
            name={props.tile.name}
            w={props.w}
            h={props.h}
            eager={props.eager}
            nameCard={false}
          />
          <div class="art-scrim" />
          <div class="art-brand">
            <Show when={props.tile.logo && !logoBroken() && tileLogo() !== null} fallback={<span class="art-name">{props.tile.name}</span>}>
              <img class="art-logo" src={tileLogo() ?? undefined} alt="" decoding="async" onError={() => setLogoBroken(true)} />
            </Show>
          </div>
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
      <Show when={cap()}>
        <p class="cap">{cap()}</p>
      </Show>
    </A>
  );
}

export function Row(props: { row: HomeRow; autofocus: boolean; root: () => Element | null; onOpen?: () => void; scope?: string }) {
  let section!: HTMLElement;
  let track: HTMLDivElement | undefined;
  const m = metrics;
  // Search shows the same catalogs as Home: keep their offsets and keys apart.
  const id = () => `${props.scope ?? "home"}:${props.row.rowId}`;
  // Continue Watching is a row of 16:9 stills; catalogs keep their shape.
  const wide = () => props.row.rowId === CONTINUE_ROW;
  const size = () => (wide() ? m().landscape : sizeOf(m(), props.row.tiles[0]?.posterShape));
  const stride = () => size()[0] + m().gap;
  const withCaption = () => props.row.tiles.some((t) => t.progress !== null);
  const bodyHeight = () => size()[1] + m().pad * 2 + (withCaption() ? m().caption : 0);
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

  return (
    <section class="row" data-row data-fgroup={`row:${id()}`} ref={section} aria-label={props.row.title}>
      <header class="row-head">
        <h2>{props.row.title}</h2>
        <span class="row-src">{props.row.rowId === CONTINUE_ROW ? "From your library" : props.row.source}</span>
        <Show when={state() === "slow" && !ready()}>
          <span class="st warn row-note">
            <i class="dot" />
            {props.row.source} is slow to answer, still trying
          </span>
        </Show>
      </header>
      <div class="row-body" style={{ height: `${bodyHeight()}px` }}>
        <Switch>
          <Match when={props.row.state.state === "failed" && !ready()}>
            <div class="row-msg">
              <CircleAlert aria-hidden class="row-msg-icon bad-ico" />
              <div>
                <p>
                  {props.row.source} could not load {props.row.title}.
                </p>
                <p class="why">{props.row.state.state === "failed" ? props.row.state.reason : ""}</p>
              </div>
              <A class="btn" href="/welcome" data-f={`fix:${id()}`}>
                <Wrench aria-hidden />
                Run setup check
              </A>
            </div>
          </Match>
          <Match when={state() === "empty"}>
            <div class="row-msg">
              <p>{props.row.source} has nothing in this catalog right now.</p>
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
                          autofocus={props.autofocus && i === 0}
                          onOpen={props.onOpen}
                          eager
                          wide={wide()}
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
