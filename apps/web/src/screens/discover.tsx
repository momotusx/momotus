import { A, useSearchParams } from "@solidjs/router";
import Puzzle from "lucide-solid/icons/puzzle";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Wrench from "lucide-solid/icons/wrench";
import { createEffect, createSignal, For, Match, on, Show, Switch, untrack } from "solid-js";
import { Grid, PullDown, Segmented, SkeletonGrid } from "../components/grid";
import { markSrc } from "../components/marks";
import { Why } from "../components/why";
import { discoverView } from "../core/client";
import type { DiscoverSnapshot } from "../core/generated/DiscoverSnapshot";
import type { Tile } from "../core/generated/Tile";
import { onProfileReset } from "../core/profiles";
import { form } from "../form";
import { defaultsWords } from "../lib/defaults";
import { extends_ } from "../lib/discover-hold";
import { listFailed, problemWords } from "../lib/words";
import "./browse.css";

// Where the page was scrolled to: the view is kept, so Back lands here too.
const pageScroll = { top: 0 };
onProfileReset(() => {
  pageScroll.top = 0;
});

/** Movies, Series and Anime are the segmented control; any other kind goes in a More pull-down,
 * so the control keeps its shape (plans/discover-v2.md 6.1). */
const MAIN = ["movie", "series", "anime"];

type Select = { kind?: string; platform?: string | null; sort?: string; genre?: string | null; decade?: string | null };

export default function Discover() {
  // Try again on Discover that could not open: a new view, as the page anew.
  const [attempt, setAttempt] = createSignal(0);
  return (
    <Show when={attempt() + 1} keyed>
      <DiscoverPage retry={() => setAttempt((n) => n + 1)} />
    </Show>
  );
}

/** The footer's words for a list: its size, its scope, and what did not answer. */
export function scopeLine(s: DiscoverSnapshot): string {
  const sort = s.sorts.find((o) => o.id === s.sort);
  const parts: string[] = [];
  if (s.scope.titles !== null) parts.push(`${s.scope.titles} ${s.scope.titles === 1 ? "title" : "titles"}`);
  else parts.push(`${s.items.length} titles`);
  if (s.scope.note && sort) parts.push(`${sort.label}, ${s.scope.note}`);
  if (s.scope.looked !== null) parts.push(`Looked through the first ${s.scope.looked}`);
  if (s.scope.pending > 0) parts.push(`${s.scope.pending} ${s.scope.pending === 1 ? "service" : "services"} still loading`);
  if (s.scope.failed > 0) parts.push(`${s.scope.failed} ${s.scope.failed === 1 ? "service" : "services"} did not answer`);
  return parts.join(" · ");
}

/** The empty line: what was asked, never a cause the data does not give. */
export function emptyLine(s: DiscoverSnapshot): string {
  const service = s.platforms.find((p) => p.id === s.platform)?.name;
  return `No ${s.genre ?? "titles"}${s.decade ? ` from the ${s.decade}` : ""}${service ? ` on ${service}` : ""} right now.`;
}

/** The value a list is ordered by, on the poster's corner: the IMDb score for the IMDb sort (even
 * with the cover's band, which can be weeks old), the year for the date sorts. */
function corner(sort: string | null, t: Tile) {
  if (sort === "imdb" && t.ratings.imdb)
    return (
      <>
        <img src={markSrc("imdb")} alt="" />
        {t.ratings.imdb}
      </>
    );
  if ((sort === "newest" || sort === "oldest") && t.releaseInfo) return <>{t.releaseInfo}</>;
  return null;
}

/** A service on the rail: its logo (TMDB's, from the core) on a rounded square, its name the label and
 * the words while the logo loads, when there is none, or when it does not load (6.6). */
function ServiceTile(props: { id: string; name: string; logo: string | null; on: boolean; pick: () => void }) {
  let el!: HTMLButtonElement;
  // The chosen service scrolls into view with room for its ring.
  createEffect(() => props.on && queueMicrotask(() => el.scrollIntoView({ inline: "nearest", block: "nearest" })));
  const [failed, setFailed] = createSignal(false);
  const [loaded, setLoaded] = createSignal(false);
  const art = () => !!props.logo && !failed();
  return (
    <button
      ref={el}
      class="svc-tile"
      classList={{ on: props.on, logo: art() && loaded() }}
      role="radio"
      aria-checked={props.on}
      aria-label={props.name}
      data-f={`discover:rail:${props.id}`}
      onClick={() => props.pick()}
    >
      <Show when={art()}>
        <img
          src={props.logo ?? ""}
          alt=""
          width="84"
          height="84"
          decoding="async"
          referrerpolicy="no-referrer"
          onLoad={() => setLoaded(true)}
          onError={() => setFailed(true)}
        />
      </Show>
      <span class="svc-name">{props.name}</span>
    </button>
  );
}

function DiscoverPage(props: { retry: () => void }) {
  const view = discoverView();
  const attach = (el: HTMLElement) => {
    if (pageScroll.top) queueMicrotask(() => el.scrollTo({ top: pageScroll.top }));
  };
  const d = () => view.state.data;
  const select = (c: Select) => void view.command({ type: "select", ...c });
  const loadMore = () => d()?.state.state === "ready" && void view.command({ type: "loadMore" });
  // A link from a title's genre chip (`?kind=series&genre=Drama`): one select with the kind and the
  // genre, which the core reads as a link and starts from a clean slate (all services, any year,
  // the kind's first sort). Taken from the URL once and dropped with a replace.
  const [params, setParams] = useSearchParams();
  if (params.kind || params.genre) pageScroll.top = 0;
  createEffect(() => {
    const kind = typeof params.kind === "string" ? params.kind : undefined;
    const genre = typeof params.genre === "string" ? params.genre : undefined;
    if (!kind && !genre) return;
    setParams({ kind: undefined, genre: undefined }, { replace: true });
    select({ kind: kind ?? d()?.kind ?? undefined, genre: genre ?? null });
  });
  // Try again after a failed page (or a failed list): loadMore asks it again. Its button stays,
  // reading "Trying again", until the answer, so the remote keeps its place.
  const [retrying, setRetrying] = createSignal(false);
  createEffect(on(() => d()?.state.state, (st) => st !== "loading" && setRetrying(false), { defer: true }));
  const retry = () => {
    if (retrying()) return;
    setRetrying(true);
    view.command({ type: "loadMore" }).catch(() => setRetrying(false));
  };
  const retryButton = (key: string) => (
    <button class="btn" data-f={key} aria-disabled={retrying()} onClick={retry}>
      <RefreshCw aria-hidden />
      {retrying() ? "Trying again" : "Try again"}
    </button>
  );
  // While a new list loads, the one before stays, dimmed and inert (6.1 Loading); the skeleton is
  // for a first open only.
  const [previous, setPrevious] = createSignal<Tile[]>([]);
  createEffect(() => {
    const s = d();
    // A copy: the store's array is reconciled in place, and emptied when the next list starts.
    if (s && s.items.length > 0) setPrevious([...s.items]);
  });
  // A late list that reorders the grid waits on TV while focus is in it, so nothing moves under the
  // viewer's focus: "More titles arrived. Show" applies it, and so does focus leaving the grid (6.1).
  let gridWrap: HTMLDivElement | undefined;
  const [shown, setShown] = createSignal<Tile[]>([]);
  const [held, setHeld] = createSignal(false);
  const focusInGrid = () => form() === "tv" && !!gridWrap?.contains(document.activeElement);
  createEffect(() => {
    // Copies: the store's array is reconciled in place, so comparing it with itself would always pass.
    const items = [...(d()?.items ?? [])];
    const old = untrack(shown);
    if (extends_(old, items) || items.length === 0 || !focusInGrid()) {
      setShown(items);
      setHeld(false);
    } else setHeld(true);
  });
  const release = () => {
    setShown([...(d()?.items ?? [])]);
    setHeld(false);
  };
  const filtered = () => !!(d()?.genre || d()?.decade || d()?.platform);
  // Clear moves focus to the first control that stays before it goes (6.5).
  let filters!: HTMLDivElement;
  const clear = () => {
    const stay = filters.querySelector<HTMLElement>(".pull-pill") ?? filters.closest("main")?.querySelector<HTMLElement>(".svc-tile");
    stay?.focus();
    select({ platform: null, genre: null, decade: null });
  };

  return (
    <main class="browse page discover" data-vscroll ref={attach} onScroll={(e) => (pageScroll.top = e.currentTarget.scrollTop)}>
      <Switch>
        <Match when={view.state.status.state === "failed"}>
          <h1>Discover</h1>
          <section class="problem">
            <h2>Discover could not open</h2>
            <Why reason={(view.state.status as { reason: string }).reason} />
            <button class="btn primary" data-f="discover:retry" data-autofocus onClick={() => props.retry()}>
              <RefreshCw aria-hidden />
              Try again
            </button>
          </section>
        </Match>
        <Match when={d() && d()?.kinds.length === 0}>
          <h1>Discover</h1>
          <section class="problem">
            <h2>Nothing to browse yet</h2>
            <p>{defaultsWords.emptyDiscover}</p>
            <div class="acts">
              {/* The recommended addons to switch on, not the first-run doors. */}
              <A class="btn primary" href="/welcome?step=2&review=1" data-f="discover:setup" data-autofocus>
                <Wrench aria-hidden />
                Set up Momotus
              </A>
              <A class="btn" href="/addons" data-f="discover:addons">
                <Puzzle aria-hidden />
                Add an addon
              </A>
            </div>
          </section>
        </Match>
        <Match when={d()}>
          {(s) => (
            <>
              <div class="dv-head">
                <h1>Discover</h1>
                <div class="dv-kinds">
                  <Segmented
                    label="Kind"
                    group="kinds"
                    items={s()
                      .kinds.filter(([k]) => MAIN.includes(k))
                      .map(([value, label]) => ({ value: value as string | null, label }))}
                    selected={s().kind}
                    onSelect={(kind) => kind && select({ kind })}
                    autofocus
                  />
                  <Show when={s().kinds.some(([k]) => !MAIN.includes(k))}>
                    <PullDown
                      label="More"
                      group="kinds-more"
                      items={[
                        { value: null as string | null, label: "More" },
                        ...s()
                          .kinds.filter(([k]) => !MAIN.includes(k))
                          .map(([value, label]) => ({ value: value as string | null, label })),
                      ]}
                      selected={MAIN.includes(s().kind ?? "") ? null : s().kind}
                      onSelect={(kind) => kind && select({ kind })}
                    />
                  </Show>
                </div>
              </div>
              <Show when={s().platforms.length > 0}>
                <div class="dv-rail" role="radiogroup" aria-label="Service" data-fgroup="discover:rail">
                  <div class="dv-rail-track" data-hscroll="120">
                    <button
                      class="svc-tile"
                      role="radio"
                      aria-checked={s().platform === null}
                      classList={{ on: s().platform === null }}
                      data-f="discover:rail:all"
                      onClick={() => select({ platform: null })}
                    >
                      All
                    </button>
                    <For each={s().platforms}>
                      {(p) => (
                        <ServiceTile id={p.id} name={p.name} logo={p.logo} on={s().platform === p.id} pick={() => select({ platform: p.id })} />
                      )}
                    </For>
                  </div>
                </div>
              </Show>
              <div class="dv-filters" ref={filters}>
                <Show when={s().genreChoices.length > 0}>
                  <PullDown
                    label={s().genreLabel}
                    group="genres"
                    items={[
                      ...(s().genreRequired ? [] : [{ value: null as string | null, label: `Any ${s().genreLabel.toLowerCase()}` }]),
                      ...s().genreChoices.map((g) => ({ value: g.name as string | null, label: g.name, note: g.count === null ? undefined : String(g.count) })),
                    ]}
                    selected={s().genre}
                    onSelect={(genre) => select({ genre })}
                  />
                </Show>
                <Show when={s().decades.length > 0}>
                  <PullDown
                    label="Year"
                    group="decades"
                    items={[{ value: null as string | null, label: "Any year" }, ...s().decades.map((y) => ({ value: y as string | null, label: y }))]}
                    selected={s().decade}
                    onSelect={(decade) => select({ decade })}
                  />
                </Show>
                <Show when={filtered()}>
                  <button class="dv-clear" data-f="discover:clear" aria-label="Clear filters" onClick={clear}>
                    Clear
                  </button>
                </Show>
                <Show when={held()}>
                  <button class="dv-clear" data-f="discover:show-late" onClick={release}>
                    More titles arrived. Show
                  </button>
                </Show>
                <span class="dv-gap" />
                <Show when={s().sorts.length + s().lists.length > 0}>
                  <PullDown
                    label="Sort"
                    group="sorts"
                    items={[
                      ...s().sorts.map((o) => ({ value: o.id as string | null, label: o.label })),
                      ...s().lists.map((c) => ({ value: `list:${c.id}` as string | null, label: c.name, note: "List" })),
                    ]}
                    selected={s().sort}
                    onSelect={(sort) => sort && select({ sort })}
                  />
                </Show>
              </div>
              <Switch>
                <Match when={s().genreRequired && !s().genre}>
                  <p class="note">This list only answers for one {s().genreLabel.toLowerCase()} at a time. Pick one above.</p>
                </Match>
                <Match when={s().items.length === 0 && (s().state.state === "failed" || retrying())}>
                  <div class="discover-fail">
                    <p class="note" classList={{ bad: !retrying() }}>
                      <i class="dot" />
                      {retrying() ? "Asking again." : listFailed((s().state as { reason: string }).reason)}
                    </p>
                    {retryButton("discover:retry-catalog")}
                  </div>
                </Match>
                <Match when={s().items.length === 0 && s().state.state === "loading"}>
                  <Show when={previous().length > 0} fallback={<SkeletonGrid />}>
                    <div class="dv-dim" inert>
                      <Grid items={previous()} scope="discover-prev" />
                    </div>
                  </Show>
                </Match>
                <Match when={s().items.length === 0}>
                  <p class="note">{emptyLine(s())}</p>
                </Match>
                <Match when={true}>
                  <div
                    ref={gridWrap}
                    onFocusOut={(e) => {
                      if (held() && !gridWrap?.contains(e.relatedTarget as Node | null)) release();
                    }}
                  >
                  <Grid
                    items={shown()}
                    scope="discover"
                    onNearEnd={loadMore}
                    corner={(t) => corner(s().sort, t)}
                    // The corner shows the year for the date sorts: the line under the name does not repeat it.
                    sub={() => (s().sort === "newest" || s().sort === "oldest" ? null : undefined)}
                    footer={
                      <div class="grid-foot-row">
                        <p class="grid-foot st" classList={{ warn: s().state.state === "loading", bad: s().state.state === "failed" }}>
                          <i class="dot" />
                          <Switch fallback={scopeLine(s())}>
                            <Match when={s().state.state === "loading"}>Loading more</Match>
                            <Match when={s().state.state === "failed"}>{`Could not load more. ${problemWords((s().state as { reason: string }).reason).text}`}</Match>
                          </Switch>
                        </p>
                        <Show when={s().state.state === "failed" || retrying()}>{retryButton("discover:retry-more")}</Show>
                      </div>
                    }
                  />
                  </div>
                </Match>
              </Switch>
            </>
          )}
        </Match>
        <Match when={true}>
          <h1>Discover</h1>
          <SkeletonGrid />
        </Match>
      </Switch>
    </main>
  );
}
