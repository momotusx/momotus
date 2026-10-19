import { A, useParams, useSearchParams } from "@solidjs/router";
import BookmarkCheck from "lucide-solid/icons/bookmark-check";
import BookmarkPlus from "lucide-solid/icons/bookmark-plus";
import Check from "lucide-solid/icons/check";
import CircleAlert from "lucide-solid/icons/circle-alert";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import KeyRound from "lucide-solid/icons/key-round";
import Plus from "lucide-solid/icons/plus";
import Star from "lucide-solid/icons/star";
import Film from "lucide-solid/icons/film";
import Layers from "lucide-solid/icons/layers";
import Play from "lucide-solid/icons/play";
import { createEffect, createMemo, createSignal, For, type JSX, Match, Show, Switch } from "solid-js";
import { Backdrop, Logo } from "../components/art";
import { TrailerOverlay } from "../components/trailer";
import { type TitleView, titleView, versionsView } from "../core/client";
import type { Episode } from "../core/generated/Episode";
import type { VersionsSnapshot } from "../core/generated/VersionsSnapshot";
import { focusEl, remember } from "../focus/dom";
import { form } from "../form";
import { bytes, clock, decoded, episodeLabel, pct, playPath, runtimeMinutes, seasonName, versionsPath } from "../lib/format";
import { blockedWords, choiceWords, kindLabel, qualityText, statusText } from "../lib/words";
import "./title.css";

export default function Title() {
  const params = useParams();
  return (
    <Show when={`${params.kind}/${params.id}`} keyed>
      <TitlePage kind={decoded(params.kind)} id={decoded(params.id)} />
    </Show>
  );
}

const released = (e: Episode) => !e.released || new Date(e.released).getTime() <= Date.now();

function TitlePage(props: { kind: string; id: string }) {
  const view = titleView(props.kind, props.id);
  const header = () => view.state.header;
  const d = () => header()?.details ?? null;
  const target = () => header()?.playTarget ?? null;
  const series = () => props.kind !== "movie" && (header()?.seasons.length ?? 0) > 0;
  const [trailerOpen, setTrailerOpen] = createSignal(false);
  const [synOpen, setSynOpen] = createSignal(false);
  const [artMissing, setArtMissing] = createSignal(false);
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
    const ref = /^(.+):(\d+):(\d+)$/.exec(t.videoId);
    return versionsView({
      kind: props.kind,
      videoId: t.videoId,
      title: name,
      season: ref ? Number(ref[2]) : undefined,
      episode: ref ? Number(ref[3]) : undefined,
    });
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

  return (
    <div class="title-scr" classList={{ movie: !series(), noart: artMissing() }}>
      {/* No backdrop (none named, or it does not load): the poster, blurred
          and faint, keeps the page from opening on a blank half. */}
      <Backdrop src={d()?.background} fallback={d()?.poster} class="title-bd" onMissing={() => d() && setArtMissing(true)} />
      <Show when={trailerOpen() && d()?.trailers.length}>
        <TrailerOverlay trailers={d()?.trailers ?? []} onClose={closeTrailer} />
      </Show>
      <div class="title-main" inert={trailerOpen() || undefined}>
      <Switch>
        <Match when={view.state.status.state === "failed" || header()?.state.state === "failed"}>
          <section class="info">
            <div class="problem">
              <CircleAlert aria-hidden class="problem-icon" />
              <h2>This title could not be opened</h2>
              <p>{failure(view)}</p>
              <A class="btn primary" href="/addons" data-f="title:addons" data-autofocus>
                Check addons
              </A>
            </div>
          </section>
        </Match>
        <Match when={d()}>
          {(x) => (
            <>
              <section class="info" aria-label={x().name}>
                <Logo src={x().logo} name={x().name} class="title-logo" />
                <p class="meta">
                  {/* The rating leads, a gold star and the number, as on Home. */}
                  <Show when={x().rating}>
                    <span class="rate num" aria-label={`IMDb ${x().rating}`}>
                      <Star aria-hidden />
                      {x().rating}
                    </span>
                    <Show when={metaLine().length}>
                      <b aria-hidden="true"> · </b>
                    </Show>
                  </Show>
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
                </p>
                <Show when={x().description}>
                  <button class="syn" classList={{ open: synOpen() }} data-f="title:synopsis" aria-label="Synopsis" aria-expanded={synOpen()} onClick={() => setSynOpen(!synOpen())}>
                    {x().description}
                  </button>
                  {/* Touch has no focus to open it: a phone says there is more. */}
                  <Show when={form() === "phone" && !synOpen() && (x().description?.length ?? 0) > 140}>
                    <button class="syn-more" onClick={() => setSynOpen(true)}>
                      More
                    </button>
                  </Show>
                </Show>
                <div class="play">
                  <div class="play-row">
                    {/* When nothing can play, the way forward is a source, not
                        a Play button that can only fail. */}
                    <Show
                      when={!noVersion(versions()?.state.data ?? null)}
                      fallback={
                        <Show
                          when={versions()?.state.data?.blocked?.reason === "needsAccount"}
                          fallback={
                            <A class="btn primary big" href="/addons" data-f="title:addsource" data-autofocus>
                              <Plus aria-hidden />
                              Add a source
                            </A>
                          }
                        >
                          <A class="btn primary big" href="/debrid" data-f="title:addaccount" data-autofocus>
                            <KeyRound aria-hidden />
                            {choiceWords.blocked.needsAccount.action}
                          </A>
                        </Show>
                      }
                    >
                      <Show when={target()}>
                        {(t) => (
                          <A class="btn primary big" href={playPath(props.kind, props.id, t().videoId)} data-f="title:play" data-autofocus>
                            <Play aria-hidden class="fill" />
                            {t().label}
                            <Show when={t().resumeMs}>
                              {(ms) => <small class="num">{resumeText(ms(), t().leftMs, runtimeMinutes(x().runtime))}</small>}
                            </Show>
                          </A>
                        )}
                      </Show>
                    </Show>
                    {/* Movies only (episodes carry their own marks): two labeled
                        states, the watched one filled with a check. */}
                    <Show when={props.kind === "movie"}>
                      <button
                        class="btn big watched"
                        data-f="title:watched"
                        aria-pressed={header()?.watched ?? false}
                        onClick={() => view.command({ type: "setWatched", videoId: props.id, value: !header()?.watched })}
                      >
                        <Check aria-hidden />
                        {header()?.watched ? "Watched" : "Mark as watched"}
                      </button>
                    </Show>
                  </div>
                  <VersionLine snap={versions()?.state.data ?? null} />
                </div>
                <div class="acts">
                  <Show when={target()}>
                    {(t) => (
                      <A class="btn" href={versionsPath(props.kind, props.id, t().videoId)} data-f="title:versions">
                        <Layers aria-hidden />
                        {/* One inline label, so the count never wraps apart from its word. */}
                        <span>
                          Versions
                          <Show when={versionCount(versions()?.state.data ?? null)}>
                            {(n) => <span class="num"> ({n()})</span>}
                          </Show>
                        </span>
                      </A>
                    )}
                  </Show>
                  <Show when={x().trailers.length > 0}>
                    <button class="btn" data-f="title:trailer" onClick={() => setTrailerOpen(true)}>
                      <Film aria-hidden />
                      Trailer
                    </button>
                  </Show>
                  <button
                    class="btn"
                    data-f="title:library"
                    aria-pressed={header()?.inLibrary ?? false}
                    onClick={() => view.command({ type: "setInLibrary", value: !header()?.inLibrary })}
                  >
                    <Show when={header()?.inLibrary} fallback={<BookmarkPlus aria-hidden />}>
                      <BookmarkCheck aria-hidden />
                    </Show>
                    {header()?.inLibrary ? "In Library" : "Add to Library"}
                  </button>
                </div>
                {/* Phone reads top to bottom: episodes before the details. */}
                <Show when={series() && form() === "phone"}>
                  <Episodes view={view} kind={props.kind} id={props.id} runtime={runtimeMinutes(x().runtime)} art={x().background ?? x().poster} />
                </Show>
                <dl class="facts">
                  <Fact label="Starring" value={x().cast.slice(0, 3).join(", ")} />
                  <Fact label="Directed by" value={x().directors.slice(0, 2).join(", ")} />
                  <Fact label="Runtime" value={x().runtime ? `${x().runtime}${series() ? " an episode" : ""}` : ""} />
                  <Fact label="Details from" value={x().source} />
                </dl>
              </section>
              <Show when={series() && form() !== "phone"}>
                <Episodes view={view} kind={props.kind} id={props.id} runtime={runtimeMinutes(x().runtime)} art={x().background ?? x().poster} />
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

/** Every addon answered and none offers a version this device can play. */
const noVersion = (s: VersionsSnapshot | null) => !!s && s.done && !s.recommended && s.rows.length === 0;

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

/** The chosen version in words, under Play. */
function VersionLine(props: { snap: VersionsSnapshot | null }) {
  const pick = () => props.snap?.recommended ?? null;
  const waiting = () => props.snap && !props.snap.done && !pick();
  return (
    <Switch fallback={<p class="vline muted">Finding versions</p>}>
      <Match when={pick()}>
        {(p) => {
          const st = () => statusText(p().status);
          return (
            <p class="vline">
              <span>{qualityText(p().quality)}</span>
              <For each={p().features}>{(f) => <span>{f}</span>}</For>
              <Show when={p().sizeBytes}>{(b) => <span class="num">{bytes(b())}</span>}</Show>
              <span class={`st ${st().tone}`}>
                <i class="dot" />
                {st().text}
              </span>
              <span class="muted">via {p().via}</span>
            </p>
          );
        }}
      </Match>
      <Match when={waiting()}>
        <p class="vline muted">
          <span class="st">
            <LoaderCircle aria-hidden class="spin" />
            Finding versions
          </span>
        </p>
      </Match>
      <Match when={props.snap?.done && !pick()}>
        <p class="vline muted wrap">
          <span class="st warn">
            <i class="dot" />
            {props.snap?.blocked
              ? blockedWords(props.snap.blocked.reason, props.snap.blocked.count).text
              : "No playable version found. A source is an addon that finds a playable stream for a title."}
          </span>
        </p>
      </Match>
    </Switch>
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
  const left = (e: Episode) => {
    if (e.watched) return "Watched";
    if (e.leftMs) return minutesLeft(e.leftMs);
    if (e.progress !== null) return props.runtime ? `${Math.max(1, Math.round(props.runtime * (1 - e.progress)))} min left` : `${pct(e.progress)} watched`;
    // A release is a date, not an instant: read in UTC, it stays the day it names west of UTC too.
    if (!released(e)) return `Airs ${new Date(e.released as string).toLocaleDateString("en", { month: "short", day: "numeric", year: "numeric", timeZone: "UTC" })}`;
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
      <ol class="toc" data-vscroll data-fgroup="episodes">
        <For each={props.view.state.episodes}>
          {(e) => {
            const [broken, setBroken] = createSignal(false);
            const still = () => (!broken() && e.thumbnail) || props.art;
            return (
            <li class="ep" classList={{ seen: e.watched, now: e.progress !== null, later: !released(e) }}>
              <A class="ep-main" href={playPath(props.kind, props.id, e.videoId)} data-f={`ep:${e.videoId}`}>
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
                  <span class="ep-t">{e.title || episodeLabel(e.season, e.episode)}</span>
                  <span class="ep-r num">{left(e)}</span>
                  <Show when={e.overview}>
                    <span class="ep-ov">{e.overview}</span>
                  </Show>
                </span>
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
            </li>
            );
          }}
        </For>
      </ol>
    </section>
  );
}
