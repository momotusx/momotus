/** A wrapping grid of tiles (Discover, Library) plus the filters over them:
 * segmented controls and pull-downs. Images load lazily; `onNearEnd` asks for the next page when
 * focus or the scroll position gets within two rows of the end. */

import Check from "lucide-solid/icons/check";
import ChevronDown from "lucide-solid/icons/chevron-down";
import { createEffect, createSignal, For, type JSX, on, onCleanup, onMount, Show } from "solid-js";
import type { Tile } from "../core/generated/Tile";
import { focusEl, isBackKey, remember } from "../focus/dom";
import { form } from "../form";
import { metrics, TileCard } from "./row";

export function Grid(props: {
  items: Tile[];
  scope: string;
  autofocus?: boolean;
  onNearEnd?: () => void;
  onOpen?: () => void;
  footer?: JSX.Element;
  /** A small label on the poster's corner: the value the list is ordered by (Discover). */
  corner?: (t: Tile) => JSX.Element | null;
  /** The line under a tile's name in place of the year (undefined keeps the year). */
  sub?: (t: Tile) => string | null | undefined;
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
              corner={props.corner?.(t)}
              sub={props.sub?.(t)}
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

type ChipsProps<T> = {
  label: string;
  group: string;
  items: { value: T; label: string; note?: string; a11y?: string }[];
  selected: T;
  onSelect: (v: T) => void;
  autofocus?: boolean;
};

/** A filter, one choice on at a time (design/DECISION.md, Grids): two to four choices are a
 * segmented control, more are a pull-down. */
export function Chips<T>(props: ChipsProps<T>) {
  return (
    <Show when={props.items.length > 4} fallback={<Segmented {...props} />}>
      <PullDown {...props} />
    </Show>
  );
}

/** The pill names the choice; its list opens under it on the chosen one. Up and Down move in
 * the list, Back or a press outside closes it, back on the pill. */
export function PullDown<T>(props: ChipsProps<T>) {
  const [open, setOpen] = createSignal(false);
  let wrap!: HTMLDivElement;
  let pill!: HTMLButtonElement;
  let list: HTMLDivElement | undefined;
  const key = () => `${props.group}:menu`;
  createEffect(() => remember(props.group, key()));
  const current = () => props.items.find((i) => i.value === props.selected);
  const close = () => {
    setOpen(false);
    focusEl(pill);
  };
  createEffect(
    on(open, (o) => {
      if (!o || !list) return;
      const on = list.querySelector<HTMLElement>(".pull-opt.on") ?? list.querySelector<HTMLElement>(".pull-opt");
      if (on) focusEl(on);
    }),
  );
  const outside = (e: PointerEvent) => {
    if (!wrap.contains(e.target as Node)) setOpen(false);
  };
  createEffect(() => {
    if (!open()) return;
    document.addEventListener("pointerdown", outside, true);
    onCleanup(() => document.removeEventListener("pointerdown", outside, true));
  });
  // The list keeps the remote's keys: the page under it does not move.
  const keys = (e: KeyboardEvent) => {
    const opts = [...(list?.querySelectorAll<HTMLElement>(".pull-opt") ?? [])];
    const at = opts.indexOf(document.activeElement as HTMLElement);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = opts[Math.min(opts.length - 1, Math.max(0, at + (e.key === "ArrowDown" ? 1 : -1)))];
      if (next) focusEl(next);
    } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") e.preventDefault();
    else if (isBackKey(e)) {
      e.preventDefault();
      close();
    }
  };
  return (
    <div class="chips-row" data-fgroup={props.group}>
      <span class="kicker">{props.label}</span>
      <div class="pull" ref={wrap}>
        <button
          class="chip pull-pill"
          aria-haspopup="listbox"
          aria-expanded={open()}
          aria-label={`${props.label}: ${current()?.label ?? ""}`}
          data-f={key()}
          data-autofocus={props.autofocus ? "" : undefined}
          ref={pill}
          onClick={() => setOpen((o) => !o)}
        >
          {current()?.label ?? props.label}
          <Show when={current()?.note}>
            <small>{current()?.note}</small>
          </Show>
          <ChevronDown aria-hidden />
        </button>
        <Show when={open()}>
          <div class="pull-list" role="listbox" aria-label={props.label} data-vscroll ref={list} onKeyDown={keys}>
            <For each={props.items}>
              {(item) => (
                <button
                  class="pull-opt"
                  role="option"
                  aria-selected={item.value === props.selected}
                  classList={{ on: item.value === props.selected }}
                  data-f={`${props.group}:${String(item.value)}`}
                  onClick={() => {
                    close();
                    if (item.value !== props.selected) props.onSelect(item.value);
                  }}
                >
                  <Show when={item.value === props.selected}>
                    <Check aria-hidden class="pull-check" />
                  </Show>
                  <b>{item.label}</b>
                  <Show when={item.note}>
                    <span>{item.note}</span>
                  </Show>
                </button>
              )}
            </For>
          </div>
        </Show>
      </div>
    </div>
  );
}

export function Segmented<T>(props: ChipsProps<T>) {
  // Entering the row lands on the chip that is on.
  createEffect(() => remember(props.group, `${props.group}:${String(props.selected)}`));
  // The row fades only toward a side with chips left to scroll to; the
  // sides are logical (scrollLeft runs negative in right-to-left pages).
  let track!: HTMLDivElement;
  const [before, setBefore] = createSignal(false);
  const [after, setAfter] = createSignal(false);
  const measure = () => {
    const done = Math.abs(track.scrollLeft);
    setBefore(done > 1);
    setAfter(track.scrollWidth - track.clientWidth - done > 1);
  };
  // The track and every chip are watched: chips change width when their
  // words or the font arrive.
  const ro = new ResizeObserver(() => measure());
  onCleanup(() => ro.disconnect());
  createEffect(() => {
    void props.items.length;
    queueMicrotask(() => {
      ro.observe(track);
      for (const chip of track.children) ro.observe(chip);
    });
  });
  return (
    <div class="chips-row" role="radiogroup" aria-label={props.label} data-fgroup={props.group}>
      <span class="kicker">{props.label}</span>
      <div
        class="chips-track"
        classList={{ "fade-start": before(), "fade-end": after() }}
        data-hscroll="120"
        ref={track}
        onScroll={measure}
      >
        <For each={props.items}>
          {(item, i) => (
            <button
              class="chip"
              role="radio"
              aria-checked={item.value === props.selected}
              aria-label={item.a11y}
              classList={{ on: item.value === props.selected }}
              data-f={`${props.group}:${String(item.value)}`}
              data-autofocus={props.autofocus && i() === 0 ? "" : undefined}
              onClick={() => props.onSelect(item.value)}
            >
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
