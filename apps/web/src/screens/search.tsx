/** Search as a centered command palette (design/DECISION.md, flystream's
 * pattern): strong glass over the blurred page, a pill input, a scope pill
 * and result rows with a small poster. The navigation opens it over the
 * screen you are on; /search shows the same panel as a page, for deep links.
 * The core's search view and its rows are unchanged: only how they show. */

import { A, useSearchParams } from "@solidjs/router";
import ChevronRight from "lucide-solid/icons/chevron-right";
import Film from "lucide-solid/icons/film";
import ListFilter from "lucide-solid/icons/list-filter";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import History from "lucide-solid/icons/rotate-ccw-clock";
import SearchIcon from "lucide-solid/icons/search";
import X from "lucide-solid/icons/x";
import { createEffect, createMemo, createSignal, For, Match, on, onCleanup, onMount, Show, Switch } from "solid-js";
import { Poster } from "../components/art";
import { Ratings } from "../components/ratings";
import { Why } from "../components/why";
import { liveView, online } from "../core/client";
import { searchIds } from "../lib/ids";
import type { HomeRow } from "../core/generated/HomeRow";
import type { SearchSnapshot } from "../core/generated/SearchSnapshot";
import type { Tile } from "../core/generated/Tile";
import { onProfileReset, profiles } from "../core/profiles";
import { kidsSession, kidsWords } from "../lib/kids";
import { pushBack } from "../focus/back";
import { autoFocusSoon, focusEl, mayAutoFocus } from "../focus/dom";
import { currentPath, historyDepth } from "../history";
import { ratingParts, titlePath } from "../lib/format";
import { allFailed, rowWords } from "../lib/rows";
import { kindLabel, problemWords } from "../lib/words";
import "./browse.css";
import "./search.css";
import { RouteA } from "../components/route-link";

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
// The router's address, which in Incognito is not the browser's (history.ts).
const here = currentPath;
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
  // Back closes it through the one Back stack (focus/back.ts).
  onMount(() => onCleanup(pushBack(() => closeSearch())));
  return (
    <div class="palette-layer">
      <div class="palette-scrim" aria-hidden="true" onClick={() => closeSearch()} />
      <SearchPanel dialog />
    </div>
  );
}

/** The deep link: the panel as a page under the navigation. */
export default function Search() {
  // A link from a title's director or cast member (`/search?q=Name`) sets the query once and is dropped
  // from the URL with a replace, so Back to Search keeps whatever the viewer typed since.
  const [params, setParams] = useSearchParams();
  if (typeof params.q === "string" && params.q) {
    lastQuery = params.q;
    setParams({ q: undefined }, { replace: true });
  }
  return (
    <main class="search-page">
      <SearchPanel />
    </main>
  );
}

function SearchPanel(props: { dialog?: boolean }) {
  // Try again on a search that could not open: a new panel, so a new view (one that failed to open drops its commands
  // for good). The query typed so far is `lastQuery`, sent again on mount.
  const [attempt, setAttempt] = createSignal(0);
  return (
    <Show when={attempt() + 1} keyed>
      <Panel dialog={props.dialog} retry={() => setAttempt((n) => n + 1)} />
    </Show>
  );
}

function Panel(props: { dialog?: boolean; retry: () => void }) {
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
  // Why nothing came back: no addon can search, or none matched this query.
  // A kids profile searches its own list, never an addon (plans/m11-kids.md 6.4).
  const kids = () => kidsSession(profiles());
  const none = () => (kids() ? kidsWords.searchNone : s()?.searchable === 0 ? "None of your addons can search." : `No matches for "${text()}" in your catalogs.`);
  const remember = () => void view.command({ type: "remember" });
  // Every list failed for the text on screen (no network, or one that hangs): one line and Try again in place of a
  // line per list, and the search runs again by itself once the browser is back online (QA r1, Q4 row 40).
  const failedAll = () => {
    const snap = s();
    return !!snap && snap.query === text().trim() && allFailed(rows());
  };
  const again = () => send(text());
  createEffect(on(online, (on) => on && failedAll() && again(), { defer: true }));
  // One status region, there from the start, so a screen reader hears each change: Searching, the count, why
  // there is none, or why the search could not open. The visible lines below are not live: this is their voice.
  const status = () => {
    if (view.state.status.state === "failed") return "Search could not open.";
    const snap = s();
    if (!searching()) return "";
    if (!snap || snap.query !== text() || loading()) return "Searching";
    if (count() > 0) return `${count()} ${count() === 1 ? "result" : "results"}`;
    return none();
  };
  // The palette is modal and what is behind it is inert: Tab wraps at its first and last control instead of leaving
  // for the browser's own.
  const wrapTab = (e: KeyboardEvent & { currentTarget: HTMLElement }) => {
    if (!props.dialog || e.key !== "Tab") return;
    const items = [...e.currentTarget.querySelectorAll<HTMLElement>('a[href], button:not(:disabled), input:not(:disabled), [tabindex]:not([tabindex="-1"])')].filter((el) => el.offsetWidth > 0 && !el.closest("[inert]"));
    const at = items.indexOf(document.activeElement as HTMLElement);
    const to = e.shiftKey ? (at <= 0 ? items.at(-1) : null) : at === items.length - 1 || at < 0 ? items[0] : null;
    if (!to) return;
    e.preventDefault();
    to.focus();
  };

  // A search that could not open puts a remote on its Try again: the field has nothing to search with. Try again is
  // then the screen's first focus too, so a first focus that comes after the failure (a late frame) lands on it.
  const failed = () => view.state.status.state === "failed";
  createEffect(on(failed, (f) => f && autoFocusSoon(() => document.querySelector<HTMLElement>('[data-f="search:retry"]')), { defer: true }));

  let input!: HTMLInputElement;
  onMount(() => {
    // Opened again by Back: the shell restores the result that was opened.
    if (props.dialog && !reopened) queueMicrotask(() => focusEl(input));
    reopened = false;
  });

  return (
    <section class="palette glass strong" classList={{ dialog: props.dialog }} role={props.dialog ? "dialog" : undefined} aria-modal={props.dialog ? "true" : undefined} aria-label="Search" onKeyDown={wrapTab}>
      <p class="sr" role="status">
        {status()}
      </p>
      <label class="field">
        <SearchIcon aria-hidden />
        <span class="sr">Search</span>
        <input
          ref={input}
          type="search"
          value={text()}
          placeholder={kids() ? kidsWords.searchField : "Search titles"}
          autocomplete="off"
          spellcheck={false}
          enterkeyhint="search"
          data-f="search:input"
          data-autofocus={failed() ? undefined : ""}
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
            <div class="none-search">
              <p class="st bad palette-note">
                <i class="dot" />
                Search could not open.
              </p>
              <Why class="palette-note" reason={(view.state.status as { reason: string }).reason} raw={(view.state.status as { raw?: string }).raw} />
              <button class="btn" data-f="search:retry" data-autofocus onClick={() => props.retry()}>
                <RefreshCw aria-hidden />
                Try again
              </button>
            </div>
          </Match>
          <Match when={!searching()}>
            <Show when={s()?.recent.length} fallback={<p class="palette-note">{kids() ? "Type a title from your list." : "Type a title. Results come from every addon with a search catalog."}</p>}>
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
          {/* Nothing came back: no addon can search (core `searchable`) or none matched. Either way the
              way on is more addons. */}
          <Match when={s() && s()?.query === text() && !loading() && rows().length === 0}>
            <div class="palette-note none-search">
              <p>{none()}</p>
              <Show when={!kids()}>
                <A class="btn" href="/recommended-addons" data-f="search:addons">
                  Recommended addons
                  <ChevronRight aria-hidden />
                </A>
              </Show>
            </div>
          </Match>
          <Match when={failedAll()}>
            <div class="none-search">
              <p class="st bad palette-note">
                <i class="dot" />
                {rowWords.searchFailed} {problemWords((rows().find((r) => r.state.state === "failed")?.state as { reason: string }).reason).text}
              </p>
              <button class="btn" data-f="search:again" onClick={again}>
                <RefreshCw aria-hidden />
                Try again
              </button>
            </div>
          </Match>
          <Match when={true}>
            <Show when={topGone() && !current()}>
              <section class="hits top" aria-label="Top results">
                <h3 class="hits-head">
                  <span class="kicker">Top results</span>
                </h3>
                <div class="hits-body">
                  <p class="palette-note">No top results for this search.</p>
                </div>
              </section>
            </Show>
            <For each={rows()}>
              {(row) => (
                <Show when={!current() || scoped(row).length > 0}>
                  <section class="hits" classList={{ top: row.rowId === TOP }} aria-label={row.title}>
                    <h3 class="hits-head">
                      <span class="kicker">{row.title}</span>
                    </h3>
                    <div class="hits-body">
                      <Switch>
                        <Match when={row.state.state === "failed" && row.tiles.length === 0}>
                          <p class="st bad palette-note">
                            <i class="dot" />
                            {rowWords.searchFailed} {row.state.state === "failed" ? problemWords(row.state.reason).text : ""}
                          </p>
                        </Match>
                        <Match when={row.tiles.length === 0}>
                          <Show when={row.state.state === "slow"}>
                            <p class="st warn palette-note">
                              <i class="dot" />
                              {rowWords.slow}
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
    <RouteA class="hit" href={titlePath(t().kind, t().id)} data-f={props.fkey} onClick={() => props.onOpen()}>
      {/* No poster, or not yet: a film mark on the surface, never squeezed words. */}
      <span class="hit-art" aria-hidden="true">
        <Film class="hit-ph" />
        {/* The plain poster: at this size a cover's drawn scores cannot be read, and the line beside it says them. */}
        <Poster src={t().posterFallback ?? t().poster} fallback={t().posterFallback ? t().poster : null} name={t().name} w={64} h={96} nameCard={false} />
      </span>
      <span class="hit-text">
        <b>{t().name}</b>
        <span class="hit-meta num">
          <span>{kindLabel(t().kind)}</span>
          <Show when={t().releaseInfo}>
            <i aria-hidden="true">·</i>
            <span>{t().releaseInfo}</span>
          </Show>
          <Show when={ratingParts(t().ratings).length}>
            <i aria-hidden="true">·</i>
          </Show>
          <Ratings ratings={t().ratings} sep={() => <i aria-hidden="true">·</i>} />
        </span>
      </span>
      <ChevronRight aria-hidden class="hit-chev" />
    </RouteA>
  );
}
