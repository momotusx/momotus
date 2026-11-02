import { A, useNavigate, useParams, useSearchParams } from "@solidjs/router";
import BookmarkCheck from "lucide-solid/icons/bookmark-check";
import BookmarkPlus from "lucide-solid/icons/bookmark-plus";
import CalendarClock from "lucide-solid/icons/calendar-clock";
import Check from "lucide-solid/icons/check";
import ChevronLeft from "lucide-solid/icons/chevron-left";
import CircleAlert from "lucide-solid/icons/circle-alert";
import CircleX from "lucide-solid/icons/circle-x";
import Film from "lucide-solid/icons/film";
import Layers from "lucide-solid/icons/layers";
import Play from "lucide-solid/icons/play";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import RotateCcw from "lucide-solid/icons/rotate-ccw";
import Volume2 from "lucide-solid/icons/volume-2";
import VolumeX from "lucide-solid/icons/volume-x";
import WifiOff from "lucide-solid/icons/wifi-off";
import { createEffect, createMemo, createSignal, For, type JSX, Match, on, onCleanup, onMount, Show, Switch } from "solid-js";
import { Backdrop, Logo } from "../components/art";
import { controlAria, downloadAct, downloadAdd, DownloadMark, DownloadMenu, downloadsHere, ITEM_ICON, itemEntries, kidsProfile, knownKids, type MenuAsk, touchFirst } from "../components/download";
import { SynopsisDialog } from "../components/synopsis";
import { TitleScores } from "../components/title-scores";
import { KeyedPage } from "../components/keyed-page";
import { Why } from "../components/why";
import { leftOut, RemovalNote, removeSoon, Row } from "../components/row";
import { separators } from "../components/separators";
import { TrailerOverlay } from "../components/trailer";
import { muted, playTrailer, setMuted, type TrailerControl, trailerPlaying } from "../components/yt-player";
import { type LiveView, rawOf, type TitleView, titleView, versionsView } from "../core/client";
import type { CastMember } from "../core/generated/CastMember";
import type { DownloadBadge } from "../core/generated/DownloadBadge";
import type { EmptyKind } from "../core/generated/EmptyKind";
import type { HomeRow } from "../core/generated/HomeRow";
import type { Episode } from "../core/generated/Episode";
import type { VersionsSnapshot } from "../core/generated/VersionsSnapshot";
import { focusEl, goBack, reducedMotion, remember } from "../focus/dom";
import { form, viewport } from "../form";
import { airDate, clock, decoded, episodeLabel, initials, offersLibrary, parseVideo, pct, playPath, playState, runtimeMinutes, seasonName, versionsPath } from "../lib/format";
import { capableNow, coverSize, headerOnTvNow, meteredNow, previewsOn, REVEAL_MS, trailerAllowed, walkNow } from "../lib/trailers";
import { blockedWords, choiceWords, emptyEntry, problemWords } from "../lib/words";
import { Stalled } from "../components/stalled";
import { deleteSeason, type DownloadItem, downloadControl, downloadWords, seriesControl, seriesMenu } from "../lib/download";
import { firstFocus } from "./title-focus";
import "./home.css";
import "./title.css";
import { setTitle } from "../incognito";
import { incognito, profiles } from "../core/profiles";
import { kidsSession, kidsWords, titleRefused } from "../lib/kids";
import { RouteA } from "../components/route-link";

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
const BACK_ONLY = new Set<EmptyKind>(["needsAccount", "kidsNothing", "noResults"]);

const released = (e: Episode) => !e.released || new Date(e.released).getTime() <= Date.now();

/** The header's trailer (plans/trailer-inline.md 3): the hero's cap, after a short settle. */
const HEADER_CAP_S = 45;
const HEADER_DELAY_MS = 300;
/** Titles whose header trailer ended, failed or was opened full screen this session: it does not play again. */
const headerSpent = new Set<string>();

function TitlePage(props: { kind: string; id: string; retry: () => void }) {
  const view = titleView(props.kind, props.id);
  const header = () => view.state.header;
  const d = () => header()?.details ?? null;
  const target = () => header()?.playTarget ?? null;
  createEffect(() => {
    const name = d()?.name;
    if (name) setTitle(`${name} · Momotus`);
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
  // Offline, the page shows what the downloads kept (plans/offline-downloads.md 4.18): no Versions, no Trailer.
  const offline = () => header()?.offline ?? false;
  // The download menu over the page (components/download.tsx); downloads show off TV, never on a kids profile.
  const [menu, setMenu] = createSignal<MenuAsk | null>(null);
  const downloads = () => downloadsHere() && !kidsProfile();
  const navigate = useNavigate();
  const keep = (videoId: string) => (item: DownloadItem) => {
    if (item === "start" || item === "again") downloadAdd(props.kind, props.id, "video", { videoId });
    else if (item === "choose" || item === "another") navigate(`${versionsPath(props.kind, props.id, videoId)}?purpose=download`);
    else if (item === "play") navigate(playPath(props.kind, props.id, videoId));
  };
  /** A video's control pressed: Download starts it, any other state opens its menu. A secondary press
   * (a right click, a long press, the menu key) on Download offers the version too. */
  const pressDownload = (from: string, title: string, videoId: string, badge: DownloadBadge | null, more = false) => {
    const c = downloadControl(badge);
    // A tap starts it, except on touch, where no secondary press reaches the version: there it opens the menu. In
    // Incognito the menu opens first too, so its note says the download stays.
    if (!c.items.length && !more && !touchFirst() && !incognito()) return keep(videoId)("start");
    const items: DownloadItem[] = c.items.length ? c.items : ["start", "choose"];
    setMenu({ title, detail: c.detail, entries: itemEntries(items, badge, keep(videoId)), from });
  };
  const openSeriesMenu = () => {
    const h = header();
    if (!h) return;
    const season = h.selectedSeason;
    const kept = view.state.episodes.filter((e) => e.season === season && e.download);
    const ids = kept.flatMap((e) => (e.download ? [e.download.id] : []));
    const run = (act: string | null) => {
      if (act === "season" && season !== null) return () => downloadAdd(props.kind, props.id, "season", { season });
      if (act === "series") return () => downloadAdd(props.kind, props.id, "series");
      // A season's downloads go only once asked (DECISION's Confirmation pattern, in the menu itself).
      if (act === "deleteSeason")
        return () =>
          setMenu({
            title: deleteSeason(season, kept.map((e) => e.download), d()?.name ?? ""),
            entries: [
              { key: "delete", label: "Delete", icon: ITEM_ICON.delete, run: () => downloadAct("downloadRemove", { ids }) },
              { key: "cancel", label: "Cancel", run: () => undefined },
            ],
            from: "title:download",
            focus: "cancel",
          });
      return null;
    };
    setMenu({
      title: d()?.name ?? "",
      entries: seriesMenu(season, h.seasonDownload, h.seriesDownload).map((m, i) => ({
        key: `${m.act ?? "line"}:${i}`,
        label: m.label,
        icon: m.act === "deleteSeason" ? ITEM_ICON.delete : m.act ? ITEM_ICON.start : undefined,
        run: run(m.act),
      })),
      from: "title:download",
    });
  };
  // The header's muted trailer under the art, as the hero's (plans/trailer-inline.md 3, 4): the same player, signals
  // and sound switch; the art fades once YouTube's start controls have gone.
  const [headerShown, setHeaderShown] = createSignal(false);
  // Offline the page plays no trailer (plans/offline-downloads.md 4.18).
  const headerId = createMemo(() => (offline() ? null : (d()?.trailers[0]?.youtubeId ?? null)));
  let headerHost!: HTMLDivElement;
  let headerVisible = true;
  let headerPlay: TrailerControl | null = null;
  const stopHeader = () => headerPlay?.stop();
  const startHeader = () => {
    const id = headerId();
    const box = headerHost.parentElement;
    if (headerPlay || !id || !box || !shown() || trailerOpen() || headerSpent.has(props.id)) return;
    const allowed = trailerAllowed({
      enabled: previewsOn(),
      visible: headerVisible && !document.hidden,
      reducedMotion: reducedMotion(),
      metered: meteredNow(),
      walk: walkNow(),
      // The header covers its art on every form; a TV plays it above its own memory floor (plans/trailer-inline.md 4).
      layout: "cover",
      capable: form() === "tv" ? headerOnTvNow() : capableNow(),
      hasTrailer: true,
      incognito: incognito(),
    });
    if (!allowed) return;
    // ponytail: sized once at the start, as the hero's; a window resized while it plays keeps the first size.
    const size = coverSize(box.offsetWidth, box.offsetHeight);
    Object.assign(headerHost.style, { width: `${size.width}px`, height: `${size.height}px` });
    let reveal: ReturnType<typeof setTimeout> | undefined;
    headerPlay = playTrailer({
      host: headerHost,
      id,
      capS: HEADER_CAP_S,
      onPlaying: () => {
        reveal = setTimeout(() => setHeaderShown(true), REVEAL_MS);
      },
      onTime: () => undefined,
      onEnd: (why) => {
        clearTimeout(reveal);
        setHeaderShown(false);
        headerPlay = null;
        if (why !== "stopped") headerSpent.add(props.id);
        // The sound switch goes with it: its focus goes to Trailer.
        if (document.activeElement?.closest('[data-f="title:sound"], .title-mute')) {
          const button = document.querySelector<HTMLElement>('[data-f="title:trailer"]');
          if (button) focusEl(button);
        }
      },
    });
  };
  createEffect(
    on([headerId, shown], ([id, visible]) => {
      stopHeader();
      if (!id || !visible) return;
      const timer = setTimeout(startHeader, HEADER_DELAY_MS);
      onCleanup(() => clearTimeout(timer));
    }),
  );
  onMount(() => {
    // At least half the header on screen, as the hero's rule: below that it stops, and comes back from the start.
    let timer: ReturnType<typeof setTimeout> | undefined;
    const seen = new IntersectionObserver(
      ([e]) => {
        const now = !!e && e.intersectionRatio >= 0.5;
        if (now === headerVisible) return;
        headerVisible = now;
        clearTimeout(timer);
        if (now) timer = setTimeout(startHeader, HEADER_DELAY_MS);
        else {
          // Scrolled away while it played: cut by the viewer, it does not come back this session.
          if (headerPlay) headerSpent.add(props.id);
          stopHeader();
        }
      },
      { threshold: [0, 0.5, 1] },
    );
    if (headerHost.parentElement) seen.observe(headerHost.parentElement);
    onCleanup(() => {
      clearTimeout(timer);
      seen.disconnect();
      stopHeader();
    });
  });
  // Opening the trailer full screen stops the header's for good this visit.
  const openTrailer = () => {
    headerSpent.add(props.id);
    stopHeader();
    setTrailerOpen(true);
  };
  const soundSwitch = () => trailerPlaying() && !walkNow();

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

  // Offline, or in Incognito, no trailer plays (plans/batch-10-incognito.md 5.5).
  const trailerButton = () => (d()?.trailers.length ?? 0) > 0 && !offline() && !incognito();
  const first = () =>
    firstFocus({
      ready: header()?.state.state === "ready",
      play: !!target() && !playInert(),
      versions: !!target() && !offline(),
      library: offersLibrary(header()),
      trailer: trailerButton(),
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

  // A phone shows the cover first: covers are vertical, and a backdrop is often a crop of the same art. Only where
  // its 2:3 fits: a portrait phone (up to 600 px wide), held to 75 percent of the screen, losing at most an eighth at
  // its foot; a landscape phone or a tablet keeps the backdrop band.
  const coverFits = () => viewport().w <= 600 && viewport().h * 0.75 >= viewport().w * 1.5 * 0.88;
  const cover = () => (form() === "phone" && coverFits() ? d()?.cover : null) ?? null;
  const coverShown = () => !!cover() && !artMissing();

  return (
    <div class="title-scr" classList={{ movie: !series(), noart: artMissing(), cover: coverShown(), "trailer-on": headerShown() }} ref={page} data-vscroll={form() === "phone" ? undefined : ""}>
      {/* No backdrop (none named, or it does not load): the poster's upper part,
          faint, keeps the page from opening on a blank half. */}
      <Backdrop
        src={cover() ?? d()?.background}
        fallback={d()?.poster}
        class="title-bd"
        under={coverShown() ? d()?.poster : null}
        ramp={form() === "phone" ? (cover() ? "titleCover" : "titlePhone") : series() ? "title" : "titleMovie"}
        onMissing={() => d() && setArtMissing(true)}
      >
        <div class="hero-trailer" aria-hidden="true">
          <div ref={headerHost} class="hero-player" />
        </div>
      </Backdrop>
      {/* The phone's header trailer sound, at the art's top right, where nothing else sits. */}
      <Show when={soundSwitch() && form() === "phone"}>
        <button class="trailer-mute title-mute" aria-label="Trailer sound" aria-pressed={!muted()} onClick={() => setMuted(!muted())}>
          <Show when={muted()} fallback={<Volume2 aria-hidden />}>
            <VolumeX aria-hidden />
          </Show>
        </button>
      </Show>
      <Show when={trailerShown()}>
        <TrailerOverlay trailers={d()?.trailers ?? []} onClose={closeTrailer} />
      </Show>
      <Show when={synText()}>
        {(text) => <SynopsisDialog name={d()?.name ?? ""} text={text()} onClose={closeSynopsis} />}
      </Show>
      <Show when={menu()}>{(m) => <DownloadMenu ask={m()} onClose={() => setMenu(null)} />}</Show>
      <div class="title-main" inert={trailerShown() || !!synText() || !!menu() || undefined}>
      <Switch>
        <Match when={titleRefused(header()?.state)}>
          <section class="info">
            <div class="problem" role="status">
              <CircleAlert aria-hidden class="problem-icon" />
              <h2>{kidsWords.titleRefused}</h2>
              <p>{kidsWords.ask}</p>
              <div class="problem-acts">
                <button class="btn primary" data-f="title:back" data-autofocus onClick={() => goBack()}>
                  <ChevronLeft aria-hidden />
                  Back
                </button>
              </div>
            </div>
          </section>
        </Match>
        <Match when={view.state.status.state === "failed" || header()?.state.state === "failed"}>
          <section class="info">
            <div class="problem">
              <CircleAlert aria-hidden class="problem-icon" />
              <h2>This title could not be opened</h2>
              <Why reason={failure(view)} raw={rawOf(view.state.status)} />
              <div class="problem-acts">
                <button class="btn primary" data-f="title:retry" data-autofocus onClick={() => props.retry()}>
                  <RefreshCw aria-hidden />
                  Try again
                </button>
                <Show when={!kidsSession(profiles())}>
                  <A class="btn" href="/addons" data-f="title:addons">
                    Check addons
                  </A>
                </Show>
              </div>
            </div>
          </section>
        </Match>
        <Match when={d()}>
          {(x) => (
            <>
              <section class="info" aria-label={x().name} data-reveal-top>
                {/* A cover known to print its title (the core says so: metahub's do) gets no second one, only the heading
                    for screen readers; another cover keeps the logo or the name under it. Otherwise a title with a logo
                    keeps a fixed slot for it, the logo or its name at the slot's foot, so nothing under it moves whatever
                    arrives, and whenever (a cold logo takes about a second). */}
                <Show when={!(coverShown() && d()?.coverTitled)} fallback={<h1 class="sr">{x().name}</h1>}>
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
                  <TitleScores imdb={x().ratings?.imdb} scores={x().scores} imdbUrl={x().imdbUrl} links={form() !== "tv" && !incognito()} />
                </p>
                <Show when={offline()}>
                  <p class="dl-offline" role="status">
                    <WifiOff aria-hidden />
                    {downloadWords.titleOffline}
                  </p>
                </Show>
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
                        <RouteA class="btn primary big" href={playTo(t().videoId).href} state={playTo(t().videoId).state} data-f="title:play" data-autofocus={first() === "title:play" ? "" : undefined}>
                          <Play aria-hidden class="fill" />
                          {t().label}
                          <Show when={t().resumeMs}>
                            {(ms) => <small class="num">{resumeText(ms(), t().leftMs, runtimeMinutes(x().runtime))}</small>}
                          </Show>
                        </RouteA>
                      )}
                    </Show>
                    {/* Part way in: the same video from its beginning (not in a Back-only state: it
                        would open the same dead end). */}
                    <Show when={!playInert() && target()?.resumeMs ? target() : null}>
                      {(t) => (
                        <RouteA class="btn big again" href={playPath(props.kind, props.id, t().videoId)} state={{ startOver: true }} data-f="title:startover" aria-label="Start over" title="Start over">
                          <RotateCcw aria-hidden />
                          <span classList={{ sr: form() === "phone" }}>Start over</span>
                        </RouteA>
                      )}
                    </Show>
                  </div>
                </div>
                <div class="acts">
                  <Show when={!offline() && target()}>
                    {(t) => (
                      <RouteA class="btn" href={versionsPath(props.kind, props.id, t().videoId)} data-f="title:versions" data-autofocus={first() === "title:versions" ? "" : undefined}>
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
                      </RouteA>
                    )}
                  </Show>
                  <Show when={trailerButton()}>
                    <button class="btn" data-f="title:trailer" data-autofocus={first() === "title:trailer" ? "" : undefined} onClick={openTrailer}>
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
                  {/* Download last, on the phone and desktop forms (design/DECISION.md, the title's action order):
                      a film's own control, a series' menu of seasons. A kids profile only sees "Downloaded". */}
                  <Show when={downloads() && props.kind === "movie"}>
                    {(_) => {
                      const videoId = () => target()?.videoId ?? props.id;
                      const c = () => downloadControl(header()?.download);
                      const press = (more: boolean) => pressDownload("title:download", x().name, videoId(), header()?.download ?? null, more);
                      return (
                        <button
                          class="btn dl"
                          data-f="title:download"
                          {...controlAria(c(), x().name)}
                          onClick={() => press(false)}
                          onContextMenu={(e) => {
                            e.preventDefault();
                            press(true);
                          }}
                        >
                          <DownloadMark control={c()} />
                          <span class="num">{c().label}</span>
                        </button>
                      );
                    }}
                  </Show>
                  <Show when={downloads() && series() && (header()?.seriesDownload || header()?.seasonDownload)}>
                    <button class="btn dl" data-f="title:download" {...controlAria(seriesControl(header()?.seriesDownload ?? null), x().name)} onClick={openSeriesMenu}>
                      <DownloadMark control={seriesControl(header()?.seriesDownload ?? null)} />
                      {seriesControl(header()?.seriesDownload ?? null).label}
                    </button>
                  </Show>
                  <Show when={downloadsHere() && knownKids() && header()?.download?.state === "done"}>
                    <span class="btn dl" role="status">
                      <DownloadMark control={downloadControl(header()?.download)} />
                      Downloaded
                    </span>
                  </Show>
                  {/* TV has no remove button on its tiles: the title page carries it
                      while the title is in Continue Watching (or Next Up), as the title header says from its first
                      render, so the row never grows after it is drawn. */}
                  <Show when={form() === "tv" && header()?.continuing && !leftOut(props.id)}>
                    <button class="btn" data-f="title:uncontinue" onClick={() => removeSoon({ id: props.id, name: x().name })}>
                      <CircleX aria-hidden />
                      <span class="long">Remove from Continue Watching</span>
                    </button>
                  </Show>
                  {/* The header trailer's sound, last in the row so nothing moves when it comes and goes. */}
                  <Show when={soundSwitch() && form() !== "phone"}>
                    <button class="btn" data-f="title:sound" aria-pressed={!muted()} onClick={() => setMuted(!muted())}>
                      <Show when={muted()} fallback={<Volume2 aria-hidden />}>
                        <VolumeX aria-hidden />
                      </Show>
                      Sound
                    </button>
                  </Show>
                </div>
                {/* Undo for a removal from Continue Watching, which TV makes here. */}
                <Show when={form() === "tv"}>
                  <RemovalNote />
                </Show>
                <VersionNote target={!!target()} noVideo={playInert() && !target()} view={versions()} />
                {/* Phone reads top to bottom: episodes before the details. */}
                <Show when={series() && form() === "phone"}>
                  <Episodes view={view} kind={props.kind} id={props.id} runtime={runtimeMinutes(x().runtime)} art={x().background ?? x().poster} downloads={downloads()} offline={offline()} onDownload={pressDownload} />
                </Show>
                <Show when={form() === "phone"}>
                  <Similar view={view} />
                  <RatedCover src={x().ratedPoster} />
                </Show>
                <Show when={form() === "phone"}>
                  <Cast cast={x().cast} />
                </Show>
                <dl class="facts">
                  <Show when={form() !== "phone"}>
                    <StarringFact cast={x().cast} />
                  </Show>
                  <Fact label="Directed by" value={x().directors.slice(0, 2).join(", ")} />
                  <Fact label="Runtime" value={x().runtime ? `${x().runtime}${series() ? " an episode" : ""}` : ""} />
                </dl>
              </section>
              <Show when={series() && form() !== "phone"}>
                {/* Focus back in the episodes brings the hero back from More like this below it. */}
                <div onFocusIn={toTop}>
                  <Episodes view={view} kind={props.kind} id={props.id} runtime={runtimeMinutes(x().runtime)} art={x().background ?? x().poster} downloads={downloads()} offline={offline()} onDownload={pressDownload} />
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
            <Stalled retry={props.retry} fkey="title:stalled">
              <div class="title-skel" aria-hidden="true">
                <div class="skel" />
                <div class="skel" />
                <div class="skel" />
              </div>
            </Stalled>
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
  const row = (): HomeRow => ({ rowId: "similar", title: "More like this", source: "", state: { state: "ready" }, tiles: props.view.state.similar, ranked: false });
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

/** A person's round TMDB headshot, or their initials on a plain circle while there is none or it cannot load. */
function Face(props: { person: CastMember }): JSX.Element {
  const [broken, setBroken] = createSignal(false);
  return (
    <span class="face" aria-hidden="true">
      <Show when={props.person.photo && !broken()} fallback={initials(props.person.name)}>
        <img src={props.person.photo ?? undefined} alt="" decoding="async" loading="lazy" onError={() => setBroken(true)} />
      </Show>
    </span>
  );
}

/** Starring on a phone (plans/batch-2.md R5): a row of headshots with each name and role under it, in the page's
 * flow. */
function Cast(props: { cast: CastMember[] }): JSX.Element {
  return (
    <Show when={props.cast.length}>
      <section class="cast">
        <h2 class="kicker">Starring</h2>
        <ul>
          <For each={props.cast}>
            {(person) => (
              <li>
                <Face person={person} />
                <span class="who">{person.name}</span>
                <Show when={person.character}>
                  <span class="role">{person.character}</span>
                </Show>
              </li>
            )}
          </For>
        </ul>
      </section>
    </Show>
  );
}

/** Starring on TV and desktop: a row of the facts table as before, each of the first three names after a small
 * headshot sized to the row, so the hero's column (fixed to the safe area) is not one pixel taller. */
function StarringFact(props: { cast: CastMember[] }): JSX.Element {
  return (
    <Show when={props.cast.length}>
      <div class="starring">
        <dt class="kicker">Starring</dt>
        <dd>
          <For each={props.cast.slice(0, 3)}>
            {(person) => (
              <span class="person">
                <Face person={person} />
                <span class="who">{person.name}</span>
              </span>
            )}
          </For>
        </dd>
      </div>
    </Show>
  );
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
    return e.kind === "needsAccount" ? blockedWords("needsAccount", e.count).text : emptyEntry(e.kind).text;
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
function Episodes(props: {
  view: TitleView;
  kind: string;
  id: string;
  runtime: number | null;
  art: string | null;
  downloads: boolean;
  offline: boolean;
  onDownload: (from: string, title: string, videoId: string, badge: DownloadBadge | null, more?: boolean) => void;
}) {
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
                <RouteA class="ep-main" href={playPath(props.kind, props.id, e.videoId)} data-f={`ep:${e.videoId}`}>
                  {card()}
                </RouteA>
                {/* The row plays this episode; choosing its version or marking
                    it watched are the secondary actions beside it. */}
                <div class="ep-acts" data-fentry data-fowner={`ep:${e.videoId}`}>
                  <Show when={!props.offline}>
                    <RouteA class="ep-act" href={versionsPath(props.kind, props.id, e.videoId)} data-f={`epv:${e.videoId}`} aria-label={`Choose a version of ${episodeLabel(e.season, e.episode)}`} title="Choose a version">
                      <Layers aria-hidden />
                      {/* A phone shows the icon in a circle; the word stays for screen readers. */}
                      <span classList={{ sr: form() === "phone" }}>Choose a version</span>
                    </RouteA>
                  </Show>
                  <button
                    class="ep-act"
                    data-f={`mark:${e.videoId}`}
                    aria-pressed={e.watched}
                    title={e.watched ? "Watched" : "Mark as watched"}
                    onClick={() => props.view.command({ type: "setWatched", videoId: e.videoId, value: !e.watched })}
                  >
                    <Check aria-hidden />
                    <span classList={{ sr: form() === "phone" }}>{e.watched ? "Watched" : "Mark as watched"}</span>
                  </button>
                  {/* The download control, third (plans/offline-downloads.md 3.2). */}
                  <Show when={props.downloads}>
                    {(_) => {
                      const c = () => downloadControl(e.download);
                      const name = () => `${episodeLabel(e.season, e.episode)}${e.title ? ` ${e.title}` : ""}`;
                      const press = (more: boolean) => props.onDownload(`dl:${e.videoId}`, name(), e.videoId, e.download, more);
                      return (
                        <button
                          class="ep-act dl"
                          data-f={`dl:${e.videoId}`}
                          {...controlAria(c(), name())}
                          title={c().label}
                          onClick={() => press(false)}
                          onContextMenu={(ev) => {
                            ev.preventDefault();
                            press(true);
                          }}
                        >
                          <DownloadMark control={c()} />
                          <span class="num" classList={{ sr: form() === "phone" }}>
                            {c().label}
                          </span>
                        </button>
                      );
                    }}
                  </Show>
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
