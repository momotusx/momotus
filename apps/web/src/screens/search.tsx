/** Search as a centered command palette (design/DECISION.md, flystream's
 * pattern): strong glass over the blurred page, a pill input, a scope pill
 * and result rows with a small poster. The navigation opens it over the
 * screen you are on; /search shows the same panel as a page, for deep links.
 * The core's search view and its rows are unchanged: only how they show. */

import { A } from "@solidjs/router";
import ChevronRight from "lucide-solid/icons/chevron-right";
import Film from "lucide-solid/icons/film";
import ListFilter from "lucide-solid/icons/list-filter";
import History from "lucide-solid/icons/rotate-ccw-clock";
import SearchIcon from "lucide-solid/icons/search";
import X from "lucide-solid/icons/x";
import { createEffect, createMemo, createSignal, For, Match, onCleanup, onMount, Show, Switch } from "solid-js";
import { Poster } from "../components/art";
import { Why } from "../components/why";
import { liveView } from "../core/client";
import { searchIds } from "../lib/ids";
import type { HomeRow } from "../core/generated/HomeRow";
import type { SearchSnapshot } from "../core/generated/SearchSnapshot";
import type { Tile } from "../core/generated/Tile";
import { onProfileReset } from "../core/profiles";
import { focusEl, isBackKey, mayAutoFocus } from "../focus/dom";
import { historyDepth } from "../history";
import { ratingParts, titlePath } from "../lib/format";
import { kindLabel, problemWords } from "../lib/words";
import "./browse.css";
import "./search.css";

// The last query survives closing the palette, so reopening lands on the results.
let lastQuery = "";
// Recent searches belong to the profile that typed them.
onProfileReset(() => {
  lastQuery = "";
});

const [open, setOpen] = createSignal(false);
/** The core's "Top results" row. */
const TOP = "top";
/** The palette is over the page. */
export const searchOpen = open;
let opener: HTMLElement | null = null;

// The palette is a history entry on the page's address, so the browser's
// Back closes it instead of leaving the page under it. A result opens over
// that entry, so Back from the title lands on it and the palette opens again
// on its results, the opened result focused (as Apple TV and Stremio do).
type Entry = { _palette?: boolean; _depth?: number } | null;
const onPaletteEntry = () => !!(history.state as Entry)?._palette;
const here = () => location.pathname + location.search;
// The page the palette is over.
let paletteAt = "";
// Opened again by Back: the remembered result takes the focus, not the field.
let reopened = false;
// Registered before the app's own popstate listeners (this module loads
// first): leaving the palette's entry for its page is not a screen's Back.
window.addEventListener("popstate", (e) => {
  if (onPaletteEntry()) {
    if (!open()) {
      reopened = true;
      paletteAt = here();
      setOpen(true);
    }
    return;
  }
  if (!open()) return;
  // Forward to another page: that page's own navigation.
  if (here() !== paletteAt) return void finish(false);
  e.stopImmediatePropagation();
  finish(true);
});
// A reload on the palette's entry: the palette is gone, so is its mark.
if (onPaletteEntry()) history.replaceState({ ...(history.state as object), _palette: undefined }, "");

export function openSearch() {
  opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  history.pushState({ ...(history.state as object), _palette: true, _depth: historyDepth() + 1 }, "");
  paletteAt = here();
  setOpen(true);
}

/** Closes the palette (Close, Back, the scrim) by popping its history entry;
 * a remote or keyboard gets its place back. */
export function closeSearch() {
  if (!open()) return;
  if (!onPaletteEntry()) return finish(true);
  history.back();
}

/** The route moved to `to` (the router's path and query, read before it
 * writes its history entry): another page (a result opened over the palette,
 * or any other navigation) closes it without Back and lets the new screen
 * place focus; Back that landed on the palette's own page keeps it open. */
export function leaveSearch(to: string) {
  if (!open() || to === paletteAt) return;
  finish(false);
}

function finish(refocus: boolean) {
  setOpen(false);
  const back = opener?.isConnected ? opener : document.querySelector<HTMLElement>('[data-f="nav:/search"]');
  opener = null;
  // At once, while the page is no longer inert: a focus left lost until a
  // microtask would go to the page's first focus first, scrolling it to the top.
  if (refocus && back && mayAutoFocus()) focusEl(back);
}

/** The palette over the screen: Back, the scrim or a result closes it. */
export function SearchOverlay() {
  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!isBackKey(e)) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      closeSearch();
    };
    window.addEventListener("keydown", onKey, { capture: true });
    onCleanup(() => window.removeEventListener("keydown", onKey, { capture: true }));
  });
  return (
    <div class="palette-layer">
      <div class="palette-scrim" aria-hidden="true" onClick={() => closeSearch()} />
      <SearchPanel dialog />
    </div>
  );
}

/** The deep link: the panel as a page under the navigation. */
export default function Search() {
  return (
    <main class="search-page">
      <SearchPanel />
    </main>
  );
}

function SearchPanel(props: { dialog?: boolean }) {
  const view = liveView<SearchSnapshot>("search", {}, false, "id", searchIds);
  const [text, setText] = createSignal(lastQuery);
  const [scope, setScope] = createSignal<string | null>(null);
  const s = () => view.state.data;
  const send = (q: string) => {
    lastQuery = q;
    setText(q);
    void view.command({ type: "setQuery", query: q });
  };
  if (lastQuery) queueMicrotask(() => send(lastQuery));
  const searching = () => text().trim().length > 0;
  const rows = () => s()?.rows ?? [];
  // "Top results" (the core's row "top") holds its place for the query it
  // came with: a loading row, a short answer, or none at all (Trakt failed or
  // found nothing, and the core drops the row) keeps the height of three
  // hits, so the rows under the viewer's thumb do not move.
  const [topFor, setTopFor] = createSignal<string | null>(null);
  createEffect(() => {
    const snap = s();
    if (snap?.rows.some((r) => r.rowId === TOP)) setTopFor(snap.query);
  });
  const topGone = () => {
    const snap = s();
    return !!snap && snap.query === text() && topFor() === snap.query && !snap.rows.some((r) => r.rowId === TOP);
  };
  const loading = () => rows().some((r) => r.state.state === "loading" || r.state.state === "slow");
  // The scope pill steps through the kinds the results hold (All, Movie, Series).
  const kinds = createMemo(() => [...new Set(rows().flatMap((r) => r.tiles.map((t) => t.kind)))]);
  const current = () => {
    const k = scope();
    return k && kinds().includes(k) ? k : null;
  };
  const scoped = (r: HomeRow) => (current() ? r.tiles.filter((t) => t.kind === current()) : r.tiles);
  const scopes = () => [null, ...kinds()];
  const nextScope = () => {
    const all = scopes();
    if (all.length > 1) setScope(all[(all.indexOf(current()) + 1) % all.length] ?? null);
  };
  const scopeLabel = () => (current() ? kindLabel(current() as string) : "All");
  const count = () => rows().reduce((n, r) => n + scoped(r).length, 0);
  const remember = () => void view.command({ type: "remember" });

  let input!: HTMLInputElement;
  onMount(() => {
    // Opened again by Back: the shell restores the result that was opened.
    if (props.dialog && !reopened) queueMicrotask(() => focusEl(input));
    reopened = false;
  });

  return (
    <section class="palette glass strong" classList={{ dialog: props.dialog }} role={props.dialog ? "dialog" : undefined} aria-modal={props.dialog ? "true" : undefined} aria-label="Search">
      <label class="field">
        <SearchIcon aria-hidden />
        <span class="sr">Search</span>
        <input
          ref={input}
          type="search"
          value={text()}
          placeholder="Search titles"
          autocomplete="off"
          spellcheck={false}
          enterkeyhint="search"
          data-f="search:input"
          data-autofocus
          onInput={(e) => send(e.currentTarget.value)}
        />
      </label>
      {/* One way in from the field: the scope pill first, then the results. */}
      <div class="palette-main" data-fentry>
      <div class="palette-tools">
        <button class="scope" data-f="search:scope" aria-label={`Show ${scopeLabel()}`} aria-disabled={scopes().length < 2} onClick={nextScope}>
          <ListFilter aria-hidden />
          {scopeLabel()}
        </button>
        <Show when={searching() && count() > 0}>
          <span class="palette-count num">{`${count()} ${count() === 1 ? "result" : "results"}`}</span>
        </Show>
        {/* A way out every reader and pointer can find (Back and the scrim also close it). */}
        <Show when={props.dialog}>
          <button class="palette-close" data-f="search:close" aria-label="Close search" onClick={() => closeSearch()}>
            <X aria-hidden />
            Close
          </button>
        </Show>
      </div>
      <div class="palette-body" data-vscroll data-fgroup="hits">
        <Switch>
          <Match when={view.state.status.state === "failed"}>
            <p class="st bad palette-note">
              <i class="dot" />
              Search could not open.
            </p>
            <Why class="palette-note" reason={(view.state.status as { reason: string }).reason} />
          </Match>
          <Match when={!searching()}>
            <Show when={s()?.recent.length} fallback={<p class="palette-note">Type a title. Results come from every addon with a search catalog.</p>}>
              <section class="recent">
                <h2 class="kicker">Recent searches</h2>
                <div class="chips-track wrap" data-fgroup="recent">
                  <For each={s()?.recent ?? []}>
                    {(q) => (
                      <button class="chip" data-f={`recent:${q}`} onClick={() => send(q)}>
                        <History aria-hidden />
                        {q}
                      </button>
                    )}
                  </For>
                </div>
              </section>
            </Show>
          </Match>
          {/* No installed addon has a search catalog: nothing could match, so
              the way on is an addon that searches (core `searchable`). */}
          <Match when={s() && s()?.query === text() && !loading() && rows().length === 0 && s()?.searchable === 0}>
            <div class="palette-note none-search">
              <p>None of your addons can search.</p>
              <A class="btn" href="/recommended-addons" data-f="search:addons">
                Recommended addons
                <ChevronRight aria-hidden />
              </A>
            </div>
          </Match>
          <Match when={s() && s()?.query === text() && !loading() && rows().length === 0}>
            <p class="palette-note">No matches for "{text()}" in your catalogs.</p>
          </Match>
          <Match when={true}>
            <Show when={topGone() && !current()}>
              <section class="hits top" aria-label="Top results">
                <h3 class="hits-head">
                  <span class="kicker">Top results</span>
                  <span class="hits-src">Trakt</span>
                </h3>
                <div class="hits-body">
                  <p class="palette-note">No top results for this search.</p>
                </div>
              </section>
            </Show>
            <For each={rows()}>
              {(row) => (
                <Show when={!current() || scoped(row).length > 0}>
                  <section class="hits" classList={{ top: row.rowId === TOP }} aria-label={`${row.title}, ${row.source}`}>
                    <h3 class="hits-head">
                      <span class="kicker">{row.title}</span>
                      <span class="hits-src">{row.source}</span>
                    </h3>
                    <div class="hits-body">
                      <Switch>
                        <Match when={row.state.state === "failed" && row.tiles.length === 0}>
                          <p class="st bad palette-note">
                            <i class="dot" />
                            {row.source} could not search {row.title}. {row.state.state === "failed" ? problemWords(row.state.reason).text : ""}
                          </p>
                        </Match>
                        <Match when={row.tiles.length === 0}>
                          <Show when={row.state.state === "slow"}>
                            <p class="st warn palette-note">
                              <i class="dot" />
                              {row.source} is slow to answer, still trying
                            </p>
                          </Show>
                          <div class="hit-wait" aria-hidden="true">
                            <For each={[0, 1, 2]}>{() => <div class="skel hit-skel" />}</For>
                          </div>
                        </Match>
                        <Match when={true}>
                          {/* Keyed by row and title: Top results may repeat a title a
                              row below shows, and one key on two hits sends the
                              remote back up to the first. The core keeps its rows
                              as first shown, so a hit never changes rows. */}
                          <For each={scoped(row)}>{(t) => <Hit tile={t} fkey={`hit:${row.rowId}:${t.id}`} onOpen={remember} />}</For>
                        </Match>
                      </Switch>
                    </div>
                  </section>
                </Show>
              )}
            </For>
          </Match>
        </Switch>
      </div>
      </div>
    </section>
  );
}

/** A result: a small poster, the name, then "Movie · 2021 · ★ 8.0". */
/** A result opens over the palette's history entry: Back from the title
 * returns to the results. */
function Hit(props: { tile: Tile; fkey: string; onOpen: () => void }) {
  const t = () => props.tile;
  return (
    <A class="hit" href={titlePath(t().kind, t().id)} data-f={props.fkey} onClick={() => props.onOpen()}>
      {/* No poster, or not yet: a film mark on the surface, never squeezed words. */}
      <span class="hit-art" aria-hidden="true">
        <Film class="hit-ph" />
        <Poster src={t().poster} name={t().name} w={64} h={96} nameCard={false} />
      </span>
      <span class="hit-text">
        <b>{t().name}</b>
        <span class="hit-meta num">
          <span>{kindLabel(t().kind)}</span>
          <Show when={t().releaseInfo}>
            <i aria-hidden="true">·</i>
            <span>{t().releaseInfo}</span>
          </Show>
          <For each={ratingParts(t().ratings)}>
            {(p) => (
              <>
                <i aria-hidden="true">·</i>
                <span class="rate" aria-label={p.spoken}>
                  {p.text}
                </span>
              </>
            )}
          </For>
        </span>
      </span>
      <ChevronRight aria-hidden class="hit-chev" />
    </A>
  );
}
