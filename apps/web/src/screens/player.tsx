import { A, useNavigate, useParams, useSearchParams } from "@solidjs/router";
import ArrowLeft from "lucide-solid/icons/arrow-left";
import AudioLines from "lucide-solid/icons/audio-lines";
import Captions from "lucide-solid/icons/captions";
import Check from "lucide-solid/icons/check";
import CircleAlert from "lucide-solid/icons/circle-alert";
import Gauge from "lucide-solid/icons/gauge";
import Layers from "lucide-solid/icons/layers";
import ListVideo from "lucide-solid/icons/list-video";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import Pause from "lucide-solid/icons/pause";
import Play from "lucide-solid/icons/play";
import RotateCcw from "lucide-solid/icons/rotate-ccw";
import RotateCw from "lucide-solid/icons/rotate-cw";
import SkipForward from "lucide-solid/icons/skip-forward";
import { createEffect, createMemo, createSignal, For, Match, on, onCleanup, onMount, Show, Switch, untrack } from "solid-js";
import { Backdrop } from "../components/art";
import { TraktMark } from "../components/trakt-mark";
import { liveView, prefsView } from "../core/client";
import type { PlaybackSnapshot } from "../core/generated/PlaybackSnapshot";
import type { Segment } from "../core/generated/Segment";
import type { SegmentKind } from "../core/generated/SegmentKind";
import type { SubtitleTrack } from "../core/generated/SubtitleTrack";
import { videoContext } from "../core/video";
import { autoFocus, goBack } from "../focus/dom";
import { createVideoEngine } from "../player/video-engine";
import { loadedSource, sourceStart } from "../lib/failover";
import { bytes, clock, decoded, episodeLabel, groupFailures, playPath, seasonName, titlePath, versionsPath } from "../lib/format";
import { byLanguage, trackDetail } from "../lib/subtitles";
import { blockedWords, languageName, qualityCell, qualityText, subtitleNote } from "../lib/words";
import "./player.css";

const SPEEDS = [1, 1.25, 1.5, 2, 0.75];
const HIDE_MS = 4000;
const PROGRESS_MS = 5000;
// Up Next appears this many seconds before the end.
const NEXT_AT = 20;
const SKIP: Record<SegmentKind, string> = { intro: "Skip intro", recap: "Skip recap", credits: "Skip credits", preview: "Skip preview" };

export default function Player() {
  const params = useParams();
  const [query] = useSearchParams();
  return (
    <Show when={`${params.kind}/${params.id}/${params.videoId}/${query.key ?? ""}`} keyed>
      <PlayerPage
        kind={decoded(params.kind)}
        id={decoded(params.id)}
        videoId={decoded(params.videoId)}
        chosen={typeof query.key === "string" ? query.key : null}
        binge={typeof query.binge === "string" ? query.binge : undefined}
      />
    </Show>
  );
}

function PlayerPage(props: { kind: string; id: string; videoId: string; chosen: string | null; binge?: string }) {
  const navigate = useNavigate();
  const ctx = videoContext(props.kind, props.id, props.videoId, props.binge);
  const prefs = prefsView();
  const versions = () => ctx.versions()?.state.data ?? null;

  // The version to play: the one picked on Versions, else the pick once it
  // settles (every addon answered, or a short wait after the first pick).
  const [settled, setSettled] = createSignal(false);
  createEffect(() => {
    const v = versions();
    if (!v?.recommended || settled()) return;
    if (v.done || v.frozen) setSettled(true);
    else {
      const t = setTimeout(() => setSettled(true), 3000);
      onCleanup(() => clearTimeout(t));
    }
  });
  const key = createMemo<string | null>((prev) => prev ?? props.chosen ?? (settled() ? (versions()?.recommended?.key ?? null) : null), null);

  const playback = createMemo(() => {
    const view = ctx.versions();
    const sub = view?.sub();
    const k = key();
    if (!view || sub === null || sub === undefined || !k) return null;
    return untrack(() =>
      liveView<PlaybackSnapshot>("playback", {
        versionsSub: sub,
        key: k,
        kind: props.kind,
        itemId: props.id,
        videoId: props.videoId,
        name: ctx.details()?.name,
        poster: ctx.details()?.poster ?? undefined,
        season: ctx.ref?.season,
        episode: ctx.ref?.episode,
      }),
    );
  });
  const snap = () => playback()?.state.data ?? null;
  const url = () => (snap()?.phase === "ready" ? snap()?.url : null) ?? null;
  const playing = () => [...(versions()?.recommended ? [versions()?.recommended] : []), ...(versions()?.rows ?? [])].find((r) => r?.key === snap()?.key) ?? null;

  const [osd, setOsd] = createSignal(true);
  const [panel, setPanel] = createSignal<"subs" | "audio" | null>(null);
  // The core's subtitle in use, if any.
  const [track, setTrack] = createSignal<{ url: string; lang: string } | null>(null);
  const [subError, setSubError] = createSignal<string | null>(null);
  const [ended, setEnded] = createSignal(false);
  let hideTimer: ReturnType<typeof setTimeout> | undefined;
  // The source the engine plays, named in failOver so the core ignores a late error.
  const loaded = loadedSource(snap);

  // Whatever decodes the picture (player/engine.ts); the screen only talks to it.
  const engine = createVideoEngine({
    onPlay: () => wake(),
    onPause: () => {
      setOsd(true);
      report();
    },
    onEnded: () => onEnded(),
    onError: (why) => void playback()?.command(loaded.failOver(why)),
    onLoaded: () => sendDuration(),
    onSubtitleError: (lang) => setSubError(`Could not load ${languageName(lang)} subtitles from the core`),
  });
  const { time, duration, paused, buffered, started, rate } = engine;
  // Paused by the viewer (not before the first frame, not at the end card).
  const pausedNow = () => started() && paused() && !ended();

  const wake = () => {
    setOsd(true);
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      if (!paused() && !panel()) setOsd(false);
    }, HIDE_MS);
  };

  const report = () => {
    const p = playback();
    if (!p || duration() <= 0) return;
    void p.command({ type: "progress", positionMs: Math.round(engine.position() * 1000), durationMs: Math.round(duration() * 1000), paused: paused() });
  };

  // The page says which engine draws the picture; with mpv under it, the
  // page turns transparent while the player is open (player.css).
  onMount(() => {
    document.documentElement.dataset.engine = engine.kind;
    onCleanup(() => delete document.documentElement.dataset.engine);
  });

  onMount(() => {
    wake();
    const timer = setInterval(() => !paused() && report(), PROGRESS_MS);
    // While the controls are hidden, the first press only shows them.
    const onKey = (e: KeyboardEvent) => {
      const back = e.key === "Escape" || e.key === "Backspace" || e.key === "GoBack" || e.keyCode === 10009 || e.keyCode === 461;
      if (e.key === "MediaPlayPause" || e.key === "MediaPlay" || e.key === "MediaPause") {
        e.preventDefault();
        toggle();
        return;
      }
      if (back && panel()) {
        e.preventDefault();
        e.stopImmediatePropagation();
        setPanel(null);
        return;
      }
      // An end or no-version card owns the screen: keys work on it as usual.
      if (ended() || document.querySelector(".endcard")) return;
      // Skip and Up Next stay on screen with the controls hidden: Enter on
      // them acts at once instead of only waking the controls.
      const onScreen = (document.activeElement as HTMLElement | null)?.closest(".skip-wrap, .next");
      if (!osd() && !back && !(onScreen && e.key === "Enter")) {
        e.preventDefault();
        e.stopImmediatePropagation();
      }
      wake();
    };
    window.addEventListener("keydown", onKey, { capture: true });
    onCleanup(() => {
      clearInterval(timer);
      clearTimeout(hideTimer);
      window.removeEventListener("keydown", onKey, { capture: true });
      report();
      engine.stop();
    });
  });

  // A new source (first start or failover) keeps the position.
  const start = sourceStart();
  createEffect(
    on(url, (u, prev) => {
      if (!u) {
        if (prev) start.stop(engine.position());
        return;
      }
      loaded.load();
      engine.load(u, prev ? engine.position() : start.next((snap()?.resumeMs ?? 0) / 1000));
    }),
  );

  const toggle = () => {
    engine.toggle();
    wake();
  };
  const seek = (delta: number) => {
    if (!duration()) return;
    engine.seek(Math.min(Math.max(0, engine.position() + delta), duration() - 0.5));
    wake();
  };
  const cycleSpeed = () => engine.setRate(SPEEDS[(SPEEDS.indexOf(rate()) + 1) % SPEEDS.length] ?? 1);

  // The core serves every subtitle as UTF-8 WebVTT on a loopback URL.
  const showSubtitle = (t: SubtitleTrack | null) => {
    setSubError(null);
    const sub = t ? { url: t.url, lang: t.lang } : null;
    setTrack(sub);
    engine.setSubtitle(sub);
  };
  const pickSubtitle = (t: SubtitleTrack | null) => {
    setPanel(null);
    showSubtitle(t);
  };

  // The viewer's own pick, or Off, wins over the core's start pick for the
  // rest of this play, and the core remembers it for the title.
  let chose = false;
  const chooseSubtitle = (t: SubtitleTrack | null) => {
    chose = true;
    void playback()?.command({ type: "subtitleChosen", url: t?.url ?? null });
    pickSubtitle(t);
  };
  // The core's start pick (`autoSubtitle`, plans/m13-subtitle-languages.md
  // 6): turned on once the list settles, unless the viewer chose already.
  createEffect(() => {
    const auto = snap()?.autoSubtitle;
    if (!auto || chose || untrack(track)) return;
    const t = snap()?.subtitles.find((s) => s.url === auto);
    if (t) showSubtitle(t);
  });

  // A file the core could not serve plays as an empty track and comes back
  // as a note. Only the picked track is fetched, so a new note is about it:
  // say so, and turn it off when it left the list.
  let notesSeen = 0;
  createEffect(() => {
    const notes = snap()?.subtitleNotes ?? [];
    const fresh = notes.length > notesSeen ? notes[notes.length - 1] : undefined;
    notesSeen = notes.length;
    const picked = untrack(track);
    if (!fresh || !picked) return;
    setSubError(subtitleNote(fresh));
    if (!snap()?.subtitles.some((t) => t.url === picked.url)) {
      setTrack(null);
      engine.setSubtitle(null);
    }
  });

  const remaining = () => Math.max(0, duration() - time());
  const endsAt = () => {
    // Nothing ends once every source failed.
    if (!duration() || snap()?.phase === "failed") return null;
    const at = new Date(Date.now() + (remaining() / rate()) * 1000);
    return at.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  };
  // The core looks up skip segments once it knows the length of the version
  // that plays; a failover is a new version (new key), so it hears again.
  const durationSent = new Set<string>();
  const sendDuration = () => {
    const p = playback();
    const key = snap()?.key;
    if (!p || !key || durationSent.has(key) || duration() <= 0) return;
    durationSent.add(key);
    void p.command({ type: "duration", key, durationMs: Math.round(duration() * 1000) });
  };

  // The segment under the playhead, active on [startMs, endMs).
  const segment = (): Segment | null => {
    const t = time() * 1000;
    return snap()?.segments.find((s) => t >= s.startMs && t < s.endMs) ?? null;
  };
  // Credits that run to the end of the file lead into Up Next rather than a seek.
  const creditsToEnd = (s: Segment) => s.kind === "credits" && duration() > 0 && s.endMs >= duration() * 1000 - 1000;
  // Where Skip credits was pressed; seeking back before it hides Up Next again.
  const [skippedAt, setSkippedAt] = createSignal<number | null>(null);
  // Manual only: the viewer presses Skip; the player never jumps by itself.
  const skip = (s: Segment) => {
    if (creditsToEnd(s)) {
      if (next()) setSkippedAt(s.startMs);
      else engine.seek(Math.max(0, duration() - 0.05));
    } else {
      engine.seek(s.endMs / 1000);
    }
    wake();
  };

  const next = () => snap()?.next ?? null;
  // Up Next: from the release-matched credits start when the core knows it,
  // else the last NEXT_AT seconds, or at once after Skip credits.
  const showNext = () => {
    if (!next() || duration() <= 60 || ended()) return false;
    const skipped = skippedAt();
    if (skipped !== null && time() * 1000 >= skipped) return true;
    const at = snap()?.nextAtMs;
    return at !== null && at !== undefined ? time() * 1000 >= at : remaining() <= NEXT_AT;
  };
  const showSkip = () => !!segment() && !showNext() && !panel() && !ended();
  // Skip takes focus when it appears, like Up Next; when it goes, focus goes
  // back to play and pause if it was on Skip.
  createEffect(
    on(
      () => (showSkip() ? `${segment()?.kind}:${segment()?.startMs}` : null),
      (now, was) => {
        if (now && now !== was) {
          queueMicrotask(() => autoFocus(document.querySelector<HTMLElement>('[data-f="player:skip"]')));
        } else if (!now && was && !document.activeElement?.closest(".player-scr [data-f]")) {
          queueMicrotask(() => autoFocus(document.querySelector<HTMLElement>('[data-f="player:toggle"]')));
        }
      },
    ),
  );
  const nextHref = () => {
    const n = next();
    if (!n) return null;
    const binge = snap()?.bingeGroup;
    return `${playPath(props.kind, props.id, n.videoId)}${binge ? `?binge=${encodeURIComponent(binge)}` : ""}`;
  };
  // Up Next takes focus when it appears, like Skip buttons on other players.
  createEffect(
    on(showNext, (shown, was) => {
      if (shown && !was) {
        setOsd(true);
        queueMicrotask(() => autoFocus(document.querySelector<HTMLElement>('[data-f="next:play"]')));
      }
    }),
  );
  // Opening a panel moves focus into it (the press asked for it); closing
  // returns to the button that opened it.
  createEffect(
    on(panel, (p, was) => {
      const target = p
        ? (document.querySelector<HTMLElement>(".panel .opt.on") ?? document.querySelector<HTMLElement>(".panel .opt"))
        : was
          ? document.querySelector<HTMLElement>(`[data-f="player:${was}"]`)
          : null;
      if (target) queueMicrotask(() => autoFocus(target));
    }),
  );

  // The end of the file: an end card with the next step, never a dead end.
  const onEnded = () => {
    report();
    const href = nextHref();
    if (href && prefs.state.data?.autoplayNext) navigate(href, { replace: true });
    else {
      setOsd(false);
      setEnded(true);
    }
  };
  createEffect(
    on(ended, (e) => {
      if (e) queueMicrotask(() => autoFocus(document.querySelector<HTMLElement>(".endcard [data-autofocus]")));
    }),
  );
  const playAgain = () => {
    setEnded(false);
    setSkippedAt(null);
    engine.seek(0);
    engine.play();
  };
  // The engine's own tracks (player/engine.ts): mpv lists the file's audio
  // and subtitle tracks; the <video> engine at most the audio ones.
  const audioTracks = () => engine.tracks().filter((t) => t.kind === "audio");
  const fileSubs = () => engine.tracks().filter((t) => t.kind === "sub");
  // Audio: the track the engine reports, else what the playing version says.
  const audioText = () => {
    const picked = audioTracks().find((t) => t.selected);
    if (picked && audioTracks().length > 1) return picked.lang ? languageName(picked.lang) : picked.label;
    const lang = playing()?.languages[0];
    const channels = playing()?.features.find((f) => /^\d\.\d$|Atmos/.test(f));
    return [lang ? languageName(lang) : null, channels].filter(Boolean).join(" ") || "Default";
  };
  // A subtitle inside the file replaces the core's one, and the other way round.
  const pickFileSub = (id: string) => {
    // A track in the file is the viewer's choice too.
    chose = true;
    pickSubtitle(null);
    engine.selectTrack("sub", id);
  };

  const pct = (x: number) => `${duration() ? Math.min(100, (x / duration()) * 100) : 0}%`;
  const status = (): { tone: string; text: string; busy: boolean } | null => {
    const v = ctx.versions();
    if (v?.state.status.state === "failed") return { tone: "bad", text: (v.state.status as { reason: string }).reason, busy: false };
    const s = snap();
    if (s?.phase === "ready") return null;
    if (s?.phase === "failed") return { tone: "bad", text: "None of the sources could be played", busy: false };
    if (s?.phase === "resolving") {
      const r = playing();
      return { tone: "warn", text: r ? `Starting ${qualityText(r.quality)}${r.sizeBytes ? `, ${bytes(r.sizeBytes)}` : ""} from ${r.via}` : "Starting", busy: true };
    }
    const vs = versions();
    if (vs?.blocked) return { tone: "bad", text: blockedWords(vs.blocked.reason, vs.blocked.count).text, busy: false };
    if (vs?.done && !vs.recommended && !vs.rows.length) return { tone: "bad", text: "No playable version found", busy: false };
    if (vs?.pendingAddons.length) return { tone: "warn", text: `Asking ${vs.pendingAddons.join(", ")} for versions`, busy: true };
    return { tone: "warn", text: ctx.details() ? "Choosing the best version" : "Opening", busy: true };
  };

  return (
    <div
      class="player-scr"
      data-scheme="dark"
      classList={{ idle: !osd() && !!url(), started: started(), paused: pausedNow() }}
      onPointerMove={wake}
      onPointerDown={(e) => {
        // While the controls are hidden, a click only brings them back.
        if (!osd()) {
          e.preventDefault();
          wake();
        }
      }}
    >
      <Backdrop src={ctx.details()?.background} class="player-bd" dim />
      {engine.view}
      {/* Paused, the picture steps back and the title says what it is, as
          the Apple TV app, Netflix and Infuse do. */}
      <div class="pdim" aria-hidden="true" />
      <div class="osd-top" />
      <div class="osd-bot" />

      <header class="ptitle">
        <h1>{ctx.details()?.name ?? ""}</h1>
        <Show when={ctx.label()}>{(l) => <p>{l()}</p>}</Show>
        <Show when={pausedNow() && (ctx.episode()?.overview ?? ctx.details()?.description)}>{(t) => <p class="psyn">{t()}</p>}</Show>
        <TraktMark state={snap()?.trakt} />
      </header>
      <Show when={endsAt()}>{(t) => <p class="clock num">Ends at {t()}</p>}</Show>

      <Show when={status()}>
        {(s) => (
          <Show
            when={!s().busy}
            fallback={
              <section class="pstate" role="status">
                <p class={`st ${s().tone}`}>
                  <LoaderCircle aria-hidden class="spin" />
                  {s().text}
                </p>
              </section>
            }
          >
            {/* Nothing plays: a centered card with the way on, never a
                black screen with a line of text. */}
            <section class="endcard" role="alert">
              <div class="end-body">
                <p class="st bad">
                  <CircleAlert aria-hidden />
                  {s().text}
                </p>
                <Show when={snap()?.attempts.length}>
                  <ul class="attempts">
                    <For each={groupFailures(snap()?.attempts ?? [])}>
                      {(a) => (
                        <li>
                          <b>{a.via}</b>
                          <span>{a.words}</span>
                        </li>
                      )}
                    </For>
                  </ul>
                </Show>
                <div class="acts">
                  <A class="btn primary big" href={versionsPath(props.kind, props.id, props.videoId)} data-f="player:versions" data-autofocus>
                    <Layers aria-hidden />
                    Choose a version
                  </A>
                  <button class="btn big" data-f="player:back" onClick={goBack}>
                    <ArrowLeft aria-hidden />
                    Back
                  </button>
                </div>
              </div>
            </section>
          </Show>
        )}
      </Show>

      <Show when={ended()}>
        <section class="endcard" aria-label="Finished">
          <div class="end-body">
            <Show
              when={next()}
              fallback={
                <>
                  <p class="kicker">Finished</p>
                  <h2>{ctx.details()?.name}</h2>
                  <div class="acts">
                    <button class="btn primary big" data-f="end:back" data-autofocus onClick={goBack}>
                      <ArrowLeft aria-hidden />
                      Back to {ctx.details()?.name ?? "the title"}
                    </button>
                    <button class="btn big" data-f="end:again" onClick={playAgain}>
                      <RotateCcw aria-hidden />
                      Play again
                    </button>
                  </div>
                </>
              }
            >
              {(n) => (
                <>
                  <p class="kicker">Next episode</p>
                  <h2>
                    {episodeLabel(n().season, n().episode)} {n().title}
                  </h2>
                  <div class="acts">
                    <A class="btn primary big" href={nextHref() ?? "#"} replace data-f="end:next" data-autofocus>
                      <SkipForward aria-hidden />
                      Next episode
                    </A>
                    <button class="btn big" data-f="end:again" onClick={playAgain}>
                      <RotateCcw aria-hidden />
                      Play again
                    </button>
                    <button class="btn big" data-f="end:back" onClick={goBack}>
                      <ArrowLeft aria-hidden />
                      Back to {ctx.details()?.name ?? "the title"}
                    </button>
                  </div>
                </>
              )}
            </Show>
          </div>
        </section>
      </Show>

      <Show when={subError()}>
        <p class="toast bad" role="status">
          <CircleAlert aria-hidden />
          {subError()}
        </p>
      </Show>

      <Show when={url() && !ended()}>
        <section class="osd" aria-label="Playback controls" inert={panel() !== null || undefined}>
          <div class="tl">
            <span class="num">{clock(time())}</span>
            <div
              class="scrub"
              role="slider"
              tabindex="0"
              aria-label="Position"
              aria-valuemin={0}
              aria-valuemax={Math.round(duration())}
              aria-valuenow={Math.round(time())}
              aria-valuetext={`${clock(time())} of ${clock(duration())}`}
              data-f="player:scrub"
              onKeyDown={(e) => {
                if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
                  e.preventDefault();
                  seek(e.key === "ArrowLeft" ? -10 : 10);
                }
              }}
              onClick={(e) => {
                const r = e.currentTarget.getBoundingClientRect();
                if (duration()) engine.seek(((e.clientX - r.left) / r.width) * duration());
              }}
            >
              <For each={buffered()}>{([a, b]) => <i class="buf" style={{ left: pct(a), width: `calc(${pct(b)} - ${pct(a)})` }} />}</For>
              <i class="pos" style={{ width: pct(time()) }} />
              <b class="knob" style={{ left: pct(time()) }} />
            </div>
            <span class="num">-{clock(remaining())}</span>
          </div>
          <div class="ctl">
            <div class="transport">
              <button class="round" data-f="player:back10" aria-label="Back 10 seconds" onClick={() => seek(-10)}>
                <RotateCcw aria-hidden />
                <span class="round-n num" aria-hidden="true">
                  10
                </span>
              </button>
              <button class="round main" data-f="player:toggle" data-autofocus aria-label={paused() ? "Play" : "Pause"} onClick={toggle}>
                <Show when={paused()} fallback={<Pause aria-hidden class="fill" />}>
                  <Play aria-hidden class="fill" />
                </Show>
              </button>
              <button class="round" data-f="player:fwd10" aria-label="Forward 10 seconds" onClick={() => seek(10)}>
                <RotateCw aria-hidden />
                <span class="round-n num" aria-hidden="true">
                  10
                </span>
              </button>
            </div>
            {/* Audio, Subtitles, Versions, Speed, then Episodes for series. */}
            <div class="pacts">
              <button class="pa" data-f="player:audio" onClick={() => setPanel(panel() ? null : "audio")} aria-expanded={panel() === "audio"}>
                <AudioLines aria-hidden />
                <b>Audio</b>
                <small>{audioText()}</small>
              </button>
              <button class="pa" data-f="player:subs" onClick={() => setPanel(panel() ? null : "subs")} aria-expanded={panel() === "subs"}>
                <Captions aria-hidden />
                <b>Subtitles</b>
                <small>
                  {track()
                    ? (snap()?.subtitles.find((t) => t.url === track()?.url)?.languageName ?? languageName(track()?.lang ?? ""))
                    : snap()?.subtitlesPending
                      ? "Searching"
                      : snap()?.subtitles.length
                        ? `Off, ${snap()?.subtitles.length} found`
                        : "None found"}
                </small>
              </button>
              <A class="pa" href={versionsPath(props.kind, props.id, props.videoId)} data-f="player:versionsact">
                <Layers aria-hidden />
                <b>Versions</b>
                <small>{playing() ? qualityCell(playing()?.quality ?? "") : ""}</small>
              </A>
              <button class="pa" data-f="player:speed" onClick={cycleSpeed}>
                <Gauge aria-hidden />
                <b>Speed</b>
                <small class="num">{rate()}x</small>
              </button>
              <Show when={ctx.ref}>
                {(r) => (
                  <A class="pa" href={titlePath(props.kind, props.id)} data-f="player:episodes">
                    <ListVideo aria-hidden />
                    <b>Episodes</b>
                    <small>{seasonName(r().season)}</small>
                  </A>
                )}
              </Show>
            </div>
          </div>
        </section>
      </Show>

      <Show when={panel() === "audio"}>
        <aside class="panel" aria-label="Audio" data-fgroup="audio">
          <h2>Audio</h2>
          <div class="panel-list" data-vscroll>
            {/* The engine's own tracks when the file has a choice (mpv lists
                every one); otherwise the one this version plays. */}
            <Show
              when={audioTracks().length > 1}
              fallback={
                <button class="opt on" data-f="audio:current" data-autofocus onClick={() => setPanel(null)}>
                  <Check aria-hidden class="opt-check" />
                  <b>{audioText()}</b>
                  <span>The track this version plays</span>
                </button>
              }
            >
              <For each={audioTracks()}>
                {(t) => (
                  <button
                    class="opt"
                    classList={{ on: t.selected }}
                    data-f={`audio:${t.id}`}
                    onClick={() => {
                      engine.selectTrack("audio", t.id);
                      setPanel(null);
                    }}
                  >
                    <Show when={t.selected}>
                      <Check aria-hidden class="opt-check" />
                    </Show>
                    <b>{t.lang ? languageName(t.lang) : t.label}</b>
                    <Show when={t.lang && t.label !== t.lang}>
                      <span>{t.label}</span>
                    </Show>
                  </button>
                )}
              </For>
            </Show>
            <p class="opt-note">Other languages come with other versions.</p>
            <A class="opt" href={versionsPath(props.kind, props.id, props.videoId)} data-f="audio:versions">
              <b>Choose a version</b>
              <span>{versions()?.rows.length ? `${versions()?.rows.length} more listed` : "See what is listed"}</span>
            </A>
          </div>
        </aside>
      </Show>

      <Show when={panel() === "subs"}>
        <aside class="panel" aria-label="Subtitles" data-fgroup="subs">
          <h2>Subtitles</h2>
          <div class="panel-list" data-vscroll>
            <button class="opt" classList={{ on: !track() }} data-f="sub:off" data-autofocus onClick={() => chooseSubtitle(null)}>
              <Show when={!track()}>
                <Check aria-hidden class="opt-check" />
              </Show>
              Off
            </button>
            {/* One section per language, preferred first, as the core orders them. */}
            <For each={byLanguage(snap()?.subtitles ?? [])}>
              {(section) => (
                <section class="sub-lang" aria-label={section.name}>
                  <h3>{section.name}</h3>
                  <For each={section.tracks}>
                    {({ track: t, index }) => (
                      <button class="opt" classList={{ on: track()?.url === t.url }} data-f={`sub:${index}`} onClick={() => chooseSubtitle(t)}>
                        <Show when={track()?.url === t.url}>
                          <Check aria-hidden class="opt-check" />
                        </Show>
                        <b>{t.name}</b>
                        <Show when={trackDetail(t)}>
                          <span>{trackDetail(t)}</span>
                        </Show>
                      </button>
                    )}
                  </For>
                </section>
              )}
            </For>
            <Show when={fileSubs().length}>
              <p class="opt-note">In the file</p>
              <For each={fileSubs()}>
                {(t) => (
                  <button class="opt" classList={{ on: t.selected && !track() }} data-f={`subfile:${t.id}`} onClick={() => pickFileSub(t.id)}>
                    <Show when={t.selected && !track()}>
                      <Check aria-hidden class="opt-check" />
                    </Show>
                    <b>{t.lang ? languageName(t.lang) : t.label}</b>
                    <Show when={t.lang && t.label !== t.lang}>
                      <span>{t.label}</span>
                    </Show>
                  </button>
                )}
              </For>
            </Show>
            <Show when={snap()?.subtitlesPending}>
              <p class="opt-note">Still asking subtitle addons</p>
            </Show>
            <For each={snap()?.subtitleNotes ?? []}>{(n) => <p class="opt-note">{subtitleNote(n)}</p>}</For>
          </div>
        </aside>
      </Show>

      <Show when={showSkip() && segment()} keyed>
        {(s) => (
          <div class="skip-wrap">
            <button class="btn skip-btn" data-f="player:skip" onClick={() => skip(s)}>
              <SkipForward aria-hidden />
              {SKIP[s.kind]}
            </button>
            <p class="skip-src">Skip data: {s.source}</p>
          </div>
        )}
      </Show>

      <Show when={showNext() && next()}>
        {(n) => (
          <aside class="next" aria-label="Up next">
            <Show when={n().thumbnail}>{(src) => <img src={src()} alt="" width={320} height={180} decoding="async" />}</Show>
            <div>
              <p class="kicker">Next episode</p>
              <h3>
                {episodeLabel(n().season, n().episode)} {n().title}
              </h3>
              <Switch>
                <Match when={prefs.state.data?.autoplayNext}>
                  <p class="num">Starts in {Math.ceil(remaining())}</p>
                </Match>
              </Switch>
              <A class="btn primary" href={nextHref() ?? "#"} replace data-f="next:play">
                <SkipForward aria-hidden />
                Play now
              </A>
            </div>
          </aside>
        )}
      </Show>
    </div>
  );
}
