/** A wrapping grid of tiles (Discover, Library) plus the chip rows that
 * filter them. Images load lazily; `onNearEnd` asks for the next page when
 * focus or the scroll position gets within two rows of the end. */

import Check from "lucide-solid/icons/check";
import { createEffect, For, type JSX, onCleanup, onMount, Show } from "solid-js";
import type { Tile } from "../core/generated/Tile";
import { remember } from "../focus/dom";
import { form } from "../form";
import { metrics, TileCard } from "./row";

export function Grid(props: {
  items: Tile[];
  scope: string;
  autofocus?: boolean;
  onNearEnd?: () => void;
  onOpen?: () => void;
  footer?: JSX.Element;
}) {
  let sentinel!: HTMLDivElement;
  let grid!: HTMLDivElement;
  const size = () => metrics().poster;
  // Native lazy loading ignores posters clipped by the page scroller until
  // they show; start them a screen ahead instead.
  onMount(() => {
    const root = form() === "phone" ? null : grid.closest("[data-vscroll]");
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (!e.isIntersecting) continue;
          (e.target as HTMLImageElement).loading = "eager";
          io.unobserve(e.target);
        }
      },
      { root, rootMargin: "100% 0px" },
    );
    createEffect(() => {
      void props.items.length;
      queueMicrotask(() => grid.querySelectorAll<HTMLImageElement>('img[loading="lazy"]').forEach((img) => io.observe(img)));
    });
    onCleanup(() => io.disconnect());
  });
  onMount(() => {
    if (!props.onNearEnd) return;
    // The page scroller clips the sentinel, so it must be the root (phones scroll the document).
    const root = form() === "phone" ? null : sentinel.closest("[data-vscroll]");
    const io = new IntersectionObserver((e) => e[0]?.isIntersecting && props.onNearEnd?.(), { root, rootMargin: "800px 0px" });
    io.observe(sentinel);
    onCleanup(() => io.disconnect());
  });
  return (
    <div class="grid-wrap">
      <div
        ref={grid}
        class="grid"
        data-fgroup={`grid:${props.scope}`}
        style={{ "grid-template-columns": `repeat(auto-fill, ${size()[0]}px)` }}
        onFocusIn={(e) => {
          const i = Number((e.target as HTMLElement).dataset.index ?? -1);
          if (props.onNearEnd && i >= props.items.length - 12) props.onNearEnd();
        }}
      >
        <For each={props.items}>
          {(t, i) => (
            <TileCard
              tile={t}
              fkey={`g:${props.scope}:${t.id}`}
              index={i()}
              w={size()[0]}
              h={size()[1]}
              autofocus={props.autofocus && i() === 0}
              onOpen={props.onOpen}
            />
          )}
        </For>
      </div>
      {props.footer}
      <div ref={sentinel} class="sentinel" aria-hidden="true" />
    </div>
  );
}

export function SkeletonGrid(props: { count?: number }) {
  const size = () => metrics().poster;
  return (
    <div class="grid" aria-hidden="true" style={{ "grid-template-columns": `repeat(auto-fill, ${size()[0]}px)` }}>
      <For each={Array.from({ length: props.count ?? 12 })}>
        {() => <div class="skel" style={{ width: `${size()[0]}px`, height: `${size()[1]}px`, "border-radius": "var(--radius-m)" }} />}
      </For>
    </div>
  );
}

/** One row of filter chips: one is on at a time. */
export function Chips<T>(props: {
  label: string;
  group: string;
  items: { value: T; label: string; note?: string }[];
  selected: T;
  onSelect: (v: T) => void;
  autofocus?: boolean;
}) {
  // Entering the row lands on the chip that is on.
  createEffect(() => remember(props.group, `${props.group}:${String(props.selected)}`));
  return (
    <div class="chips-row" role="radiogroup" aria-label={props.label} data-fgroup={props.group}>
      <span class="kicker">{props.label}</span>
      <div class="chips-track" data-hscroll="120">
        <For each={props.items}>
          {(item, i) => (
            <button
              class="chip"
              role="radio"
              aria-checked={item.value === props.selected}
              classList={{ on: item.value === props.selected }}
              data-f={`${props.group}:${String(item.value)}`}
              data-autofocus={props.autofocus && i() === 0 ? "" : undefined}
              onClick={() => props.onSelect(item.value)}
            >
              <Show when={item.value === props.selected}>
                <Check aria-hidden class="chip-check" />
              </Show>
              {item.label}
              <Show when={item.note}>
                <small>{item.note}</small>
              </Show>
            </button>
          )}
        </For>
      </div>
    </div>
  );
}
