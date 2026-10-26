import { A, useParams, useSearchParams } from "@solidjs/router";
import BookmarkCheck from "lucide-solid/icons/bookmark-check";
import BookmarkPlus from "lucide-solid/icons/bookmark-plus";
import CalendarClock from "lucide-solid/icons/calendar-clock";
import Check from "lucide-solid/icons/check";
import CircleAlert from "lucide-solid/icons/circle-alert";
import CircleX from "lucide-solid/icons/circle-x";
import Film from "lucide-solid/icons/film";
import Layers from "lucide-solid/icons/layers";
import Play from "lucide-solid/icons/play";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import RotateCcw from "lucide-solid/icons/rotate-ccw";
import { createEffect, createMemo, createSignal, For, type JSX, Match, onCleanup, Show, Switch } from "solid-js";
import { Backdrop, Logo } from "../components/art";
import { SynopsisDialog } from "../components/synopsis";
import { TitleScores } from "../components/title-scores";
import { KeyedPage } from "../components/keyed-page";
import { Why } from "../components/why";
import { askOnce, removeFromContinue, Row } from "../components/row";
import { separators } from "../components/separators";
import { TrailerOverlay } from "../components/trailer";
import { type LiveView, type TitleView, titleView, versionsView } from "../core/client";
import type { EmptyKind } from "../core/generated/EmptyKind";
import type { HomeRow } from "../core/generated/HomeRow";
import type { Episode } from "../core/generated/Episode";
import type { VersionsSnapshot } from "../core/generated/VersionsSnapshot";
import { focusEl, remember } from "../focus/dom";
import { form } from "../form";
import { airDate, clock, decoded, episodeLabel, offersLibrary, parseVideo, pct, playPath, playState, runtimeMinutes, seasonName, versionsPath } from "../lib/format";
import { blockedWords, choiceWords, problemWords } from "../lib/words";
import { firstFocus } from "./title-focus";
import "./home.css";
import "./title.css";

export default function Title() {
  const params = useParams();
  // Try again on a title that could not be opened: the page anew.
  const [attempt, setAttempt] = createSignal(0);
  return (
    <KeyedPage
      address={`${params.kind}/${params.id}/${attempt()}`}
      read={() => ({ kind: decoded(params.kind), id: decoded(params.id), retry: () => setAttempt((n) => n + 1) })}
      page={TitlePage}
    />
  );
}

// The core's empty states whose only action is Back (plans/play-every-version.md).
const BACK_ONLY = new Set<EmptyKind>(["noRoom", "needsAccount", "kidsNothing", "noResults"]);

const released = (e: Episode) => !e.released || new Date(e.released).getTime() <= Date.now();

function TitlePage(props: { kind: string; id: string; retry: () => void }) {
  const view = titleView(props.kind, props.id);
  const header = () => view.state.header;
  const d = () => header()?.details ?? null;
  const target = () => header()?.playTarget ?? null;
  const uncontinue = askOnce(() => removeFromContinue(props.id));
  createEffect(() => {
    const name = d()?.name;
    if (name) document.title = `${name} · Momotus`;
  });
  const series = () => props.kind !== "movie" && (header()?.seasons.length ?? 0) > 0;
  const [trailerOpen, setTrailerOpen] = createSignal(false);
  const [synOpen, setSynOpen] = createSignal(false);
  // What is shown and what makes the page inert are one condition, so a dialog that unmounts (its
  // text gone in a refresh) never leaves the page inert.
  const synText = () => (synOpen() && d()?.description) || null;
  const trailerShown = () => trailerOpen() && (d()?.trailers.length ?? 0) > 0;
  // Closing returns focus to what opened it: the synopsis, or the phone's More.
  let synOpener = "title:synopsis";
  const openSynopsis = (key: string) => {
    synOpener = key;
    setSynOpen(true);
  };
  const closeSynopsis = () => {
    setSynOpen(false);
    queueMicrotask(() => {
      const opener = document.querySelector<HTMLElement>(`[data-f="${synOpener}"]`);
      if (opener) focusEl(opener);
    });
  };
  const [artMissing, setArtMissing] = createSignal(false);
  // TV and desktop scroll the page below the hero for More like this; the phone scrolls the document.
  let page: HTMLDivElement | undefined;
  const toTop = () => {
    if (page && page.scrollTop > 0) page.scrollTo({ top: 0, behavior: "smooth" });
  };
  // The core asks TMDB and Trakt only while the page shows (`titleVisible`): a tab in the
  // background asks nothing, and leaving the page closes the view.
  const [shown, setShown] = createSignal(!document.hidden);
  const onVisibility = () => setShown(!document.hidden);
  document.addEventListener("visibilitychange", onVisibility);
  onCleanup(() => document.removeEventListener("visibilitychange", onVisibility));
  // TV: a series' rated cover may sit alone under the hero (no More like this to focus there), and it is never a focus
  // stop. Down with nowhere to go scrolls the page to it; Up moves focus in the hero, which brings the page back to the
  // top (`data-reveal-top`, and the episodes' `toTop`), and Up with nowhere to go brings it back too.
  let before: Element | null = null;
  const tvKeyFirst = () => {
    before = document.activeElement;
  };
  const tvKeyAfter = (e: KeyboardEvent) => {
    if (form() !== "tv" || !page || document.activeElement !== before) return;
    // Up with nowhere to go (the hero's topmost control focused) still brings the hero back.
    if (e.key === "ArrowUp" && page.scrollTop > 0) return toTop();
    // A tile of More like this has the card beside it in view already: the page stays put.
    if (e.key !== "ArrowDown" || document.activeElement?.closest(".title-more")) return;
    const cover = page.querySelector<HTMLElement>(".title-more .rated-cover");
    if (cover) cover.scrollIntoView({ block: "end", behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
  };
  window.addEventListener("keydown", tvKeyFirst, { capture: true });
  window.addEventListener("keydown", tvKeyAfter);
  onCleanup(() => {
    window.removeEventListener("keydown", tvKeyFirst, { capture: true });
    window.removeEventListener("keydown", tvKeyAfter);
  });
  createEffect(() => {
    if (view.state.status.state === "open") view.command({ type: "titleVisible", visible: shown() }).catch(() => undefined);
  });
  // Closing returns focus to the button that opened it.
  const closeTrailer = () => {
    setTrailerOpen(false);
    queueMicrotask(() => {
      const button = document.querySelector<HTMLElement>('[data-f="title:trailer"]');
      if (button) focusEl(button);
    });
  };

  // Open Versions for the play target as soon as the name is known: the
  // line under Play shows the pick, and the core warms it before Play.
  const versions = createMemo(() => {
    const t = target();
    const name = d()?.name;
    if (!t || !name) return null;
    const ref = parseVideo(props.id, t.videoId);
    return versionsView({
      kind: props.kind,
      videoId: t.videoId,
      title: name,
      season: ref?.season ?? undefined,
      episode: ref?.episode,
    });
  });

  // A Back-only state (plans/play-every-version.md, the title page's row): Play keeps its place,
  // inert, since a Play that opens a page holding only Back is a dead end; with no video at all the
  // same. On TV the first focus falls to the next action (title-focus.ts).
  const playInert = () => {
    const e = versions()?.state.data?.empty;
    return (!!e && BACK_ONLY.has(e.kind)) || (header()?.state.state === "ready" && !target() && !header()?.nextAirs);
  };

  // Where Play goes when the core says nothing plays as is (the lead, 2026-10-23): the kept version when
  // the sources are unreachable, else Versions, which holds the state's line, its action and the rows
  // that start. The title page itself never offers Settings, Try again or Choose a version.
  const playTo = (videoId: string): { href: string; state?: object } => {
    const e = versions()?.state.data?.empty;
    if (e?.kind === "unreachable" && e.key) return { href: playPath(props.kind, props.id, videoId), state: playState(e.key) };
    if (e) return { href: versionsPath(props.kind, props.id, videoId) };
    return { href: playPath(props.kind, props.id, videoId) };
  };

  const first = () =>
    firstFocus({
      ready: header()?.state.state === "ready",
      play: !!target() && !playInert(),
      versions: !!target(),
      library: offersLibrary(header()),
      trailer: (d()?.trailers.length ?? 0) > 0,
      synopsis: !!d()?.description,
    });

  const metaLine = () => {
    const x = d();
    if (!x) return [];
    const seasons = header()?.seasons.filter((s) => s > 0).length ?? 0;
    return [
      x.releaseInfo,
      x.genres.slice(0, 3).join(", ") || null,
      seasons ? `${seasons} ${seasons === 1 ? "season" : "seasons"}` : null,
      props.kind === "movie" ? x.runtime : null,
    ].filter(Boolean) as string[];
  };

  // A phone shows the cover first: covers are vertical, and a backdrop is often a crop of the same art.
  const cover = () => (form() === "phone" ? d()?.cover : null) ?? null;
  const coverShown = () => !!cover() && !artMissing();

  return (
    <div class="title-scr" classList={{ movie: !series(), noart: artMissing(), cover: coverShown() }} ref={page} data-vscroll={form() === "phone" ? undefined : ""}>
      {/* No backdrop (none named, or it does not load): the poster's upper part,
          faint, keeps the page from opening on a blank half. */}
      <Backdrop
        src={cover() ?? d()?.background}
        fallback={d()?.poster}
        class="title-bd"
        ramp={form() === "phone" ? (cover() ? "titleCover" : "titlePhone") : series() ? "title" : "titleMovie"}
        onMissing={() => d() && setArtMissing(true)}
      />
      <Show when={trailerShown()}>
        <TrailerOverlay trailers={d()?.trailers ?? []} onClose={closeTrailer} />
      </Show>
      <Show when={synText()}>
        {(text) => <SynopsisDialog name={d()?.name ?? ""} text={text()} onClose={closeSynopsis} />}
      </Show>
      <div class="title-main" inert={trailerShown() || !!synText() || undefined}>
      <Switch>
        <Match when={view.state.status.state === "failed" || header()?.state.state === "failed"}>
          <section class="info">
            <div class="problem">
              <CircleAlert aria-hidden class="problem-icon" />
              <h2>This title could not be opened</h2>
              <Why reason={failure(view)} />
              <div class="problem-acts">
                <button class="btn primary" data-f="title:retry" data-autofocus onClick={() => props.retry()}>
                  <RefreshCw aria-hidden />
                  Try again
                </button>
                <A class="btn" href="/addons" data-f="title:addons">
                  Check addons
                </A>
              </div>
            </div>
          </section>
        </Match>
        <Match when={d()}>
          {(x) => (
            <>
              <section class="info" aria-label={x().name} data-reveal-top>
                {/* A cover prints its own title: no second one over or under it, only the heading for screen readers.
                    Otherwise a title with a logo keeps a fixed slot for it, the logo or its name at the slot's foot,
                    so nothing under it moves whatever arrives, and whenever (a cold logo takes about a second). */}
                <Show when={!coverShown()} fallback={<h1 class="sr">{x().name}</h1>}>
                  <Show when={x().logo} fallback={<Logo src={null} name={x().name} class="title-logo" />}>
                    <div class="logo-slot">
                      <Logo src={x().logo} name={x().name} class="title-logo" />
                    </div>
                  </Show>
                </Show>
                <p class="meta" ref={separators}>
                  {/* DECISION's order: the year first, then the rest of the line, then the ratings row at
                      its end, each number after its service's mark; IMDb's links to its page, except on TV. */}
                  <For each={metaLine()}>
                    {(part, i) => (
                      <>
                        <Show when={i() > 0}>
                          <b aria-hidden="true"> · </b>
                        </Show>
                        <span class="num">{part}</span>
                      </>
                    )}
                  </For>
                  <TitleScores imdb={x().ratings?.imdb} scores={x().scores} imdbUrl={x().imdbUrl} links={form() !== "tv"} />
                </p>
                <Show when={x().description}>
                  {/* The full text opens on its own card (components/synopsis.tsx), never in place. */}
                  <button class="syn" data-f="title:synopsis" data-autofocus={first() === "title:synopsis" ? "" : undefined} aria-label="Synopsis, open the full text" aria-haspopup="dialog" onClick={() => openSynopsis("title:synopsis")}>
                    {x().description}
                  </button>
                  {/* Touch has no focus to open it: a phone says there is more. */}
                  <Show when={form() === "phone" && (x().description?.length ?? 0) > 140}>
                    <button class="syn-more" data-f="title:more" aria-haspopup="dialog" onClick={() => openSynopsis("title:more")}>
                      More
                    </button>
                  </Show>
                </Show>
                <div class="play">
                  <div class="play-row">
                    {/* Resume or Play is always the primary (plans/play-every-version.md, shared Step 3
                        decisions): when nothing plays here, the note under the actions says why.
                        The next episode not out yet: its day, in place of Play (header.nextAirs). A
                        series watched to its end gets "Play again from S1:E1" from the core as its
                        label. */}
                    <Show
                      when={!playInert() && target()}
                      fallback={
                        <Show when={header()?.nextAirs} fallback={<InertPlay label={target()?.label ?? "Play"} />}>
                          {(day) => (
                            <p class="airs-next" role="status">
                              <CalendarClock aria-hidden />
                              Next episode airs {airDate(day())}
                            </p>
                          )}
                        </Show>
                      }
                    >
                      {(t) => (
                        <A class="btn primary big" href={playTo(t().videoId).href} state={playTo(t().videoId).state} data-f="title:play" data-autofocus={first() === "title:play" ? "" : undefined}>
                          <Play aria-hidden class="fill" />
                          {t().label}
                          <Show when={t().resumeMs}>
                            {(ms) => <small class="num">{resumeText(ms(), t().leftMs, runtimeMinutes(x().runtime))}</small>}
                          </Show>
                        </A>
                      )}
                    </Show>
                    {/* Part way in: the same video from its beginning (not in a Back-only state: it
                        would open the same dead end). */}
                    <Show when={!playInert() && target()?.resumeMs ? target() : null}>
                      {(t) => (
                        <A class="btn big again" href={playPath(props.kind, props.id, t().videoId)} state={{ startOver: true }} data-f="title:startover" aria-label="Start over">
                          <RotateCcw aria-hidden />
                          <span classList={{ sr: form() === "phone" }}>Start over</span>
                        </A>
                      )}
                    </Show>
                  </div>
                </div>
                <div class="acts">
                  <Show when={target()}>
                    {(t) => (
                      <A class="btn" href={versionsPath(props.kind, props.id, t().videoId)} data-f="title:versions" data-autofocus={first() === "title:versions" ? "" : undefined}>
                        <Layers aria-hidden />
                        {/* One inline label, so the count never wraps apart from its word. The count comes with
                            the versions, after the first frame: under the label, unseen, the widest it shows holds
                            the button's width from the start, so the row never grows when it lands. */}
                        <span class="vlabel">
                          <span>
                            Versions
                            <Show when={versionCount(versions()?.state.data ?? null)}>
                              {(n) => <span class="num"> ({n()})</span>}
                            </Show>
                          </span>
                          <span class="vsizer" aria-hidden="true">
                            Versions <span class="num">(000)</span>
                          </span>
                        </span>
                      </A>
                    )}
                  </Show>
                  <Show when={x().trailers.length > 0}>
                    <button class="btn" data-f="title:trailer" data-autofocus={first() === "title:trailer" ? "" : undefined} onClick={() => setTrailerOpen(true)}>
                      <Film aria-hidden />
                      Trailer
                    </button>
                  </Show>
                  <Show when={offersLibrary(header())}>
                  <button
                    class="btn"
                    data-f="title:library"
                    data-autofocus={first() === "title:library" ? "" : undefined}
                    aria-pressed={header()?.inLibrary ?? false}
                    onClick={() => view.command({ type: "setInLibrary", value: !header()?.inLibrary })}
                  >
                    <Show when={header()?.inLibrary} fallback={<BookmarkPlus aria-hidden />}>
                      <BookmarkCheck aria-hidden />
                    </Show>
                    {header()?.inLibrary ? "In Library" : "Add to Library"}
                  </button>
                  </Show>
                  {/* Movies only (episodes carry their own marks): a small action beside Library, its
                      check in the circle, filled once marked (the owner, 2026-10-23). */}
                  <Show when={props.kind === "movie"}>
                    <button
                      class="btn"
                      data-f="title:watched"
                      aria-pressed={header()?.watched ?? false}
                      onClick={() => view.command({ type: "setWatched", videoId: props.id, value: !header()?.watched })}
                    >
                      <Check aria-hidden />
                      Watched
                    </button>
                  </Show>
                  {/* TV has no remove button on its tiles: the title page carries it
                      while the title is in Continue Watching (or Next Up), as the title header says from its first
                      render, so the row never grows after it is drawn. */}
                  <Show when={form() === "tv" && header()?.continuing}>
                    <button class="btn" classList={{ danger: uncontinue.ask() !== "idle" }} data-f="title:uncontinue" onClick={uncontinue.press}>
                      <CircleX aria-hidden />
                      <span class="long">
                        {uncontinue.ask() === "confirm" ? "Press again to remove" : uncontinue.ask() === "failed" ? "Not removed, try again" : "Remove from Continue Watching"}
                      </span>
                    </button>
                  </Show>
                </div>
                <VersionNote target={!!target()} noVideo={playInert() && !target()} view={versions()} />
                {/* Phone reads top to bottom: episodes before the details. */}
                <Show when={series() && form() === "phone"}>
                  <Episodes view={view} kind={props.kind} id={props.id} runtime={runtimeMinutes(x().runtime)} art={x().background ?? x().poster} />
                </Show>
                <Show when={form() === "phone"}>
                  <Similar view={view} />
                  <RatedCover src={x().ratedPoster} />
                </Show>
                <dl class="facts">
                  <Fact label="Starring" value={x().cast.slice(0, 3).join(", ")} />
                  <Fact label="Directed by" value={x().directors.slice(0, 2).join(", ")} />
                  <Fact label="Runtime" value={x().runtime ? `${x().runtime}${series() ? " an episode" : ""}` : ""} />
                </dl>
              </section>
              <Show when={series() && form() !== "phone"}>
                {/* Focus back in the episodes brings the hero back from More like this below it. */}
                <div onFocusIn={toTop}>
                  <Episodes view={view} kind={props.kind} id={props.id} runtime={runtimeMinutes(x().runtime)} art={x().background ?? x().poster} />
                </div>
              </Show>
              {/* TV and desktop: the rated cover at the hero's right for a movie; a series' episodes panel holds
                  that place, so its cover leads the body under the hero, beside More like this (Rotten Tomatoes is
                  never dropped from the page). More like this sits under the hero, where the page scrolls. */}
              <Show when={form() !== "phone"}>
                <Show when={!series()}>
                  <RatedCover src={x().ratedPoster} />
                </Show>
                <div class="title-more">
                  <Show when={series()}>
                    <RatedCover src={x().ratedPoster} />
                  </Show>
                  <Similar view={view} root={() => page ?? null} />
                </div>
              </Show>
            </>
          )}
        </Match>
        <Match when={true}>
          <section class="info" aria-busy="true">
            <div class="title-skel" aria-hidden="true">
              <div class="skel" />
              <div class="skel" />
              <div class="skel" />
            </div>
          </section>
        </Match>
      </Switch>
      </div>
    </div>
  );
}

/** RPDB's cover with the IMDb, Metacritic and Rotten Tomatoes band drawn in (plans/title-page-ratings-genres-similar.md):
 * the page's only Rotten Tomatoes, drawn only when the title has one, decoration only (never focusable), and
 * gone, not replaced, when the image fails. */
function RatedCover(props: { src: string | null }) {
  const [failed, setFailed] = createSignal(false);
  return (
    <Show when={!failed() && props.src}>
      {(src) => (
        <figure class="rated-cover">
          <img src={src()} alt="Cover with ratings" decoding="async" class="fade" onLoad={(e) => e.currentTarget.classList.add("in")} onError={() => setFailed(true)} />
        </figure>
      )}
    </Show>
  );
}

/** "More like this": the core's related titles as a Home row of 2:3 covers, no source named; absent
 * while empty (until it lands, for a kids profile, and when nothing came). */
function Similar(props: { view: TitleView; root?: () => Element | null }) {
  const row = (): HomeRow => ({ rowId: "similar", title: "More like this", source: "", state: { state: "ready" }, tiles: props.view.state.similar });
  return (
    <Show when={props.view.state.similar.length > 0}>
      <Row row={row()} root={props.root ?? (() => null)} scope="title" />
    </Show>
  );
}

function failure(view: TitleView): string {
  const s = view.state.status;
  if (s.state === "failed") return s.reason;
  const h = view.state.header?.state;
  return h?.state === "failed" ? h.reason : "";
}

function Fact(props: { label: string; value: string }): JSX.Element {
  return (
    <Show when={props.value}>
      <div>
        <dt class="kicker">{props.label}</dt>
        <dd>{props.value}</dd>
      </div>
    </Show>
  );
}

/** "36 min left" from what the file has left (Continue Watching's figure), else from the runtime,
 * else the position. */
function resumeText(resumeMs: number, leftMs: number | null, runtime: number | null): string {
  if (leftMs) return minutesLeft(leftMs);
  if (runtime) return `${Math.max(1, Math.round(runtime - resumeMs / 60_000))} min left`;
  return `from ${clock(resumeMs / 1000)}`;
}

/** Whole minutes rounded up, as the core words Continue Watching. */
const minutesLeft = (ms: number) => `${Math.max(1, Math.ceil(ms / 60_000))} min left`;

function versionCount(s: VersionsSnapshot | null): number | null {
  if (!s) return null;
  const n = s.rows.length + (s.recommended ? 1 : 0);
  return n || null;
}

/** The version status in one quiet line under the action row, as Apple's VersionNote: the core's
 * reason nothing plays here (its `empty` line, apps/shared/stream-choice-words.json), or that the
 * versions could not load (in plain words, `problemWords`). Its slot is as tall as the longest `empty`
 * line from the first paint (`role="status"`), and a longer failure is clamped to it, so nothing moves
 * when it fills. "Finding versions" is not said: Play and the Versions button carry that. */
function VersionNote(props: { target: boolean; noVideo: boolean; view: LiveView<VersionsSnapshot> | null }) {
  const words = () => {
    // ponytail: no video and nothing airing has no core state yet; it reads as noResults.
    if (props.noVideo) return choiceWords.empty.noResults.text;
    const v = props.view;
    if (!props.target || !v) return null;
    if (v.state.status.state === "failed") return `Versions could not load. ${problemWords(v.state.status.reason).text}`;
    const e = v.state.data?.empty;
    if (!e) return null;
    // Versions that need an account are said quietly, with their count and no way to an account (the
    // owner, 2026-10-23: accounts are set up in Settings only), as Android and Apple say it.
    return e.kind === "needsAccount" ? blockedWords("needsAccount", e.count).text : choiceWords.empty[e.kind].text;
  };
  return (
    <p class="vnote" role="status">
      <Show when={words()}>
        {(w) => (
          <span class="st warn">
            <i class="dot" />
            <span class="vtext">{w()}</span>
          </span>
        )}
      </Show>
    </p>
  );
}

/** Play in a Back-only state, or with no video: in its place, dimmed, not focusable, doing nothing. */
function InertPlay(props: { label: string }) {
  return (
    <span class="btn primary big" aria-disabled="true">
      <Play aria-hidden class="fill" />
      {props.label}
    </span>
  );
}

/** `art` is the series' backdrop, else its poster: an episode without a
 * still (none named, or it fails to load) shows it, never a dark box. */
function Episodes(props: { view: TitleView; kind: string; id: string; runtime: number | null; art: string | null }) {
  const header = () => props.view.state.header;
  const seasons = () => {
    const all = header()?.seasons ?? [];
    return [...all.filter((s) => s > 0), ...all.filter((s) => s === 0)];
  };
  const compact = () => seasons().length > 5;
  createEffect(() => {
    const s = header()?.selectedSeason;
    if (s !== null && s !== undefined) remember("seasons", `season:${s}`);
  });
  // The season in the URL wins over the core's default (the play target's
  // season), so Back from an episode and links from Calendar land on it.
  const [query, setQuery] = useSearchParams();
  createEffect(() => {
    const h = header();
    const want = typeof query.season === "string" ? Number(query.season) : Number.NaN;
    if (h && Number.isInteger(want) && h.selectedSeason !== want && h.seasons.includes(want)) {
      void props.view.command({ type: "selectSeason", season: want });
    }
  });
  const select = (s: number) => setQuery({ season: String(s) }, { replace: true });
  // A wheel run holds the rows' hover off until it settles (150 ms after its last event): rows sliding
  // under a still pointer would change their hover on every frame and repaint the list each time.
  const holdHover = (list: HTMLOListElement) => {
    let settle: number | undefined;
    list.addEventListener(
      "wheel",
      () => {
        list.classList.add("wheeling");
        clearTimeout(settle);
        settle = window.setTimeout(() => list.classList.remove("wheeling"), 150);
      },
      { passive: true },
    );
  };
  const left = (e: Episode) => {
    if (e.watched) return "Watched";
    if (e.leftMs) return minutesLeft(e.leftMs);
    if (e.progress !== null) return props.runtime ? `${Math.max(1, Math.round(props.runtime * (1 - e.progress)))} min left` : `${pct(e.progress)} watched`;
    // A release is a date, not an instant: read in UTC, it stays the day it names west of UTC too.
    if (!released(e)) return `Airs ${airDate(e.released as string)}`;
    return props.runtime ? `${props.runtime} min` : "";
  };
  return (
    <section class="eps" aria-label="Episodes">
      <header class="eps-head">
        <h2>Episodes</h2>
        <div class="seasons" role="tablist" aria-label="Seasons" data-fgroup="seasons" data-hscroll="80" classList={{ compact: compact() }}>
          <Show when={compact()}>
            <span class="kicker">Season</span>
          </Show>
          <For each={seasons()}>
            {(s) => (
              <button
                class="tab"
                role="tab"
                aria-controls="season-episodes"
                aria-selected={header()?.selectedSeason === s}
                classList={{ on: header()?.selectedSeason === s }}
                data-f={`season:${s}`}
                onClick={() => select(s)}
              >
                {compact() ? (s === 0 ? "Specials" : String(s)) : seasonName(s)}
              </button>
            )}
          </For>
        </div>
      </header>
      <ol class="toc" id="season-episodes" role="tabpanel" aria-label={seasonName(header()?.selectedSeason ?? 1)} data-vscroll data-fgroup="episodes" ref={holdHover}>
        <For each={props.view.state.episodes}>
          {(e) => {
            const [broken, setBroken] = createSignal(false);
            const still = () => (!broken() && e.thumbnail) || props.art;
            const card = () => (
              <>
                {/* A card: the still with its number, then the title, the time
                    and two lines of synopsis. */}
                <span class="ep-still" aria-hidden="true">
                  <Show when={still()} keyed>
                    {(src) => (
                      <img
                        src={src}
                        alt=""
                        loading="lazy"
                        decoding="async"
                        class="fade"
                        classList={{ "stand-in": src !== e.thumbnail }}
                        onLoad={(ev) => ev.currentTarget.classList.add("in")}
                        onError={() => src === e.thumbnail && setBroken(true)}
                      />
                    )}
                  </Show>
                  <b class="ep-n num">{e.episode}</b>
                  <Show when={e.watched}>
                    <span class="ep-chip">
                      <Check aria-hidden />
                      Watched
                    </span>
                  </Show>
                  <Show when={e.progress !== null}>
                    <span class="meter">
                      <i style={{ width: pct(e.progress ?? 0) }} />
                    </span>
                  </Show>
                </span>
                <span class="ep-text">
                  {/* The number is on the picture, hidden from screen readers: said here. */}
                  <span class="sr">Episode {e.episode}, </span>
                  <span class="ep-t">{e.title || episodeLabel(e.season, e.episode)}</span>
                  <span class="ep-r num">{left(e)}</span>
                  <Show when={e.overview}>
                    <span class="ep-ov">{e.overview}</span>
                  </Show>
                </span>
              </>
            );
            return (
            <li class="ep" classList={{ seen: e.watched, now: e.progress !== null, later: !released(e) }}>
              {/* An episode not out yet is a plain row with its day: nothing
                  to play, no version to choose. Reachable on TV, so the
                  remote can read it. */}
              <Show
                when={released(e)}
                fallback={
                  <div class="ep-main" data-f={`ep:${e.videoId}`} tabindex="-1" aria-disabled="true">
                    {card()}
                  </div>
                }
              >
                <A class="ep-main" href={playPath(props.kind, props.id, e.videoId)} data-f={`ep:${e.videoId}`}>
                  {card()}
                </A>
                {/* The row plays this episode; choosing its version or marking
                    it watched are the secondary actions beside it. */}
                <div class="ep-acts" data-fentry data-fowner={`ep:${e.videoId}`}>
                  <A class="ep-act" href={versionsPath(props.kind, props.id, e.videoId)} data-f={`epv:${e.videoId}`} aria-label={`Choose a version of ${episodeLabel(e.season, e.episode)}`}>
                    <Layers aria-hidden />
                    {/* A phone shows the icon in a circle; the word stays for screen readers. */}
                    <span classList={{ sr: form() === "phone" }}>Choose a version</span>
                  </A>
                  <button
                    class="ep-act"
                    data-f={`mark:${e.videoId}`}
                    aria-pressed={e.watched}
                    onClick={() => props.view.command({ type: "setWatched", videoId: e.videoId, value: !e.watched })}
                  >
                    <Check aria-hidden />
                    <span classList={{ sr: form() === "phone" }}>{e.watched ? "Watched" : "Mark as watched"}</span>
                  </button>
                </div>
              </Show>
            </li>
            );
          }}
        </For>
      </ol>
    </section>
  );
}
