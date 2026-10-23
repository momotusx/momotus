import { A, useLocation, useNavigate, useParams, useSearchParams } from "@solidjs/router";
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
import Maximize from "lucide-solid/icons/maximize";
import Minimize from "lucide-solid/icons/minimize";
import Play from "lucide-solid/icons/play";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import RotateCcw from "lucide-solid/icons/rotate-ccw";
import RotateCw from "lucide-solid/icons/rotate-cw";
import SkipForward from "lucide-solid/icons/skip-forward";
import Volume2 from "lucide-solid/icons/volume-2";
import VolumeX from "lucide-solid/icons/volume-x";
import { createEffect, createMemo, createSignal, For, Match, on, onCleanup, onMount, Show, Switch, untrack } from "solid-js";
import { Backdrop } from "../components/art";
import { EmptyActs } from "../components/empty-acts";
import { Why } from "../components/why";
import { KeyedPage } from "../components/keyed-page";
import { TraktMark } from "../components/trakt-mark";
import { liveView, op, prefsView, serverMoves } from "../core/client";
import type { EmptyVersions } from "../core/generated/EmptyVersions";
import type { PlaybackSnapshot } from "../core/generated/PlaybackSnapshot";
import type { Segment } from "../core/generated/Segment";
import type { SegmentKind } from "../core/generated/SegmentKind";
import type { SubtitleSize } from "../core/generated/SubtitleSize";
import type { SubtitleTrack } from "../core/generated/SubtitleTrack";
import { videoContext } from "../core/video";
import { autoFocusSoon, goBack, isBackKey, restoreSoon } from "../focus/dom";
import { form } from "../form";
import { backThenReplace, entryFrom, historyBase, historyDepth, markBack, ownState, takeBack } from "../history";
import { Loading, type Wait } from "../player/loading";
import { SubtitlePicker } from "../player/subtitle-picker";
import { createVideoEngine } from "../player/video-engine";
import { loadedSource, type Retry, sourceStart, TRYING_AGAIN_MS, tryingAgain } from "../lib/failover";
import { bytes, clock, decoded, startsText, episodeLabel, groupFailures, playPath, playState, seasonName, titlePath, versionsPath } from "../lib/format";
import { DELAY_SETTLE_MS } from "../lib/delay";
import { bufferShare, loadingLines, STALL_AHEAD_S, startFill } from "../lib/loading";
import { fileChoice, offSummary, pickerLanguages, startOption } from "../lib/subtitles";
import { choiceWords, emptyWords, languageName, nextVersion, qualityCell, qualityText, restartLine, subtitleNote } from "../lib/words";
import { VersionsPage } from "./versions";
import "./player.css";

const SPEEDS = [1, 1.25, 1.5, 2, 0.75];
const HIDE_MS = 4000;
const PROGRESS_MS = 5000;
// Up Next appears this many seconds before the end.
const NEXT_AT = 20;
// A wait for data this long after the first frame is a stall (a seek's short wait is not).
const STALL_MS = 500;
/** How long the loading screen waits before it offers another version (plans/play-every-version.md 2e). */
// The profile's subtitle sizes in order, as the picker steps them.
const SIZES: { size: SubtitleSize; text: string }[] = [
  { size: "small", text: "80%" },
  { size: "medium", text: "100%" },
  { size: "large", text: "125%" },
  { size: "extraLarge", text: "150%" },
];
const SKIP: Record<SegmentKind, string> = { intro: "Skip intro", recap: "Skip recap", credits: "Skip credits", preview: "Skip preview" };

// The viewer's sound for this app run: one episode's level and mute are the next one's.
const sound = { volume: 100, muted: false };

export default function Player() {
  const params = useParams();
  const [query] = useSearchParams();
  const location = useLocation();
  const chosen = () => {
    const key = (location.state as { key?: unknown } | null)?.key;
    return typeof key === "string" ? key : null;
  };
  // "Start over" on the title page: this play starts at the beginning.
  const startOver = () => (location.state as { startOver?: unknown } | null)?.startOver === true;
  // Opened on Versions that was opened on the title page.
  const viaTitle = () => (location.state as { viaTitle?: unknown } | null)?.viaTitle === true;
  // Try again on a title that could not be opened: the page opens anew.
  const [attempt, setAttempt] = createSignal(0);
  return (
    <KeyedPage
      address={`${params.kind}/${params.id}/${params.videoId}/${chosen() ?? ""}/${attempt()}`}
      read={() => ({
        kind: decoded(params.kind),
        id: decoded(params.id),
        videoId: decoded(params.videoId),
        chosen: chosen(),
        startOver: startOver(),
        viaTitle: viaTitle(),
        binge: typeof query.binge === "string" ? query.binge : undefined,
        retry: () => setAttempt((n) => n + 1),
      })}
      page={PlayerPage}
    />
  );
}

function PlayerPage(props: { kind: string; id: string; videoId: string; chosen: string | null; startOver: boolean; viaTitle: boolean; binge?: string; retry: () => void }) {
  const navigate = useNavigate();
  // The page this play was opened from, read when needed: the router renders
  // a route before it writes its history entry. Leaving the play for that
  // same page (Versions, or the title for Episodes) goes back to it; any
  // other page replaces the play in history. Either way Back from the next
  // play never reopens this one.
  const leaveFor = (path: string) => (e: MouseEvent) => {
    if (entryFrom() !== path) return;
    e.preventDefault();
    goBack();
  };
  // This entry came from a Versions page of this title (this video's, or an
  // earlier episode's or version's before autoplay or a move on replaced it).
  const fromVersions = () => !!entryFrom()?.startsWith(`${titlePath(props.kind, props.id)}/versions/`);
  // Episodes: the title, once in history. A play opened on this title's
  // Versions, itself opened on the title, goes back past Versions.
  const toTitle = (e: MouseEvent) => {
    if (!fromVersions() || !props.viaTitle || historyDepth() - 2 < historyBase()) {
      leaveFor(titlePath(props.kind, props.id))(e);
      return;
    }
    e.preventDefault();
    markBack();
    history.go(-2);
  };
  // Versions: this video's. When the play came from an earlier episode's
  // Versions (autoplay moved on), that entry gives way to this one.
  const toVersions = (e: MouseEvent) => {
    const here = versionsPath(props.kind, props.id, props.videoId);
    if (entryFrom() === here || !fromVersions()) return leaveFor(here)(e);
    e.preventDefault();
    backThenReplace(here);
  };
  // "Choose another version" while it loads (plans/play-every-version.md 2e): Versions opens over
  // the player with an entry of its own (`?versions`). The attempt keeps resolving under it; nothing
  // plays or sounds; the layer's order holds. Back closes it, and a choice on it replaces this play.
  const location = useLocation();
  const layer = () => new URLSearchParams(location.search).has("versions");
  const layerHref = () => {
    const q = new URLSearchParams(location.search);
    q.set("versions", "");
    return `${location.pathname}?${q}`;
  };
  // Opened over a stalled frame, the play pauses under it and plays again on Back.
  let resume = false;
  const openLayer = (e: MouseEvent) => {
    e.preventDefault();
    resume = started() && !paused();
    if (resume) engine.pause();
    // Marked as the app's own, so a remount (Try again) or a reload still closes it by going back.
    navigate(layerHref(), { state: { ...ownState(location.state), _layer: true } });
  };
  // ponytail: the browser's own Back (popstate) closes the layer; design-web's pushBack can take it over.
  // With no entry of ours under it, the layer gives way in place instead.
  const closeLayer = () => {
    // A row's sheet over the layer goes first; a link straight to the layer has no entry of the app's
    // under it, so it gives way in place.
    if (new URLSearchParams(location.search).has("info") || (history.state as { _layer?: boolean } | null)?._layer) return history.back();
    const q = new URLSearchParams(location.search);
    q.delete("versions");
    navigate(`${location.pathname}${q.size ? `?${q}` : ""}`, { replace: true, state: ownState(location.state) });
  };
  let picked: string | null = null;
  const pickInLayer = (key: string) => {
    picked = key;
    closeLayer();
  };
  // What a play that replaces this one keeps from it (autoplay, Up Next, a
  // move on to another version): that it was opened on Versions via the title.
  const carried = () => ({ viaTitle: props.viaTitle });
  const ctx = videoContext(props.kind, props.id, props.videoId, props.binge);
  // Back goes where the viewer came from: it names the title only when that
  // is the title page.
  const backWords = () => (entryFrom() === titlePath(props.kind, props.id) ? `Back to ${ctx.details()?.name ?? "the title"}` : "Back");
  createEffect(() => {
    const name = ctx.details()?.name;
    if (name && !layer()) document.title = `${name} · Marquee`;
  });
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
  // A version picked by hand opens once Versions lists it or is done: after
  // a reload the Versions view is new and has listed nothing yet.
  const listed = (k: string) => {
    const v = versions();
    return !!v && (v.done || v.recommended?.key === k || v.rows.some((r) => r.key === k));
  };
  const key = createMemo<string | null>(
    (prev) => prev ?? (props.chosen ? (listed(props.chosen) ? props.chosen : null) : settled() ? (versions()?.recommended?.key ?? null) : null),
    null,
  );

  // Try again on a playback that could not open opens it once more.
  const [opens, setOpens] = createSignal(0);
  const playback = createMemo(() => {
    opens();
    // The stream server took another port while the app was away: the
    // loaded item's URLs point at the old one, so the playback opens anew.
    serverMoves();
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
        season: ctx.ref?.season ?? undefined,
        episode: ctx.ref?.episode,
      }, false, "url"),
    );
  });
  const snap = () => playback()?.state.data ?? null;
  const failed = () => snap()?.phase === "failed";
  // A failed play's reason in the shared table: no room (Back only), unreachable (Try again), none plays
  // here (another version); notStarted keeps Try again and Choose another version. A hand pick with an
  // offer keeps the offer's card.
  const failedAs = () => (failed() && !snap()?.offer ? (snap()?.empty ?? null) : null);
  const openError = () => {
    const s = playback()?.state.status;
    return s?.state === "failed" ? (s as { reason: string }).reason : null;
  };
  // The title itself could not be opened (its meta addon failed): nothing to
  // choose from, so Try again (the page anew) and Back.
  const titleError = () => ctx.failure();
  // Try again as the core took it (`retries` rose): "Trying again" until the
  // play starts, and a moment at least (lib/failover.ts tryingAgain).
  const [retry, setRetry] = createSignal<Retry | null>(null);
  const [held, setHeld] = createSignal(0);
  createEffect(
    on(
      () => snap()?.retries ?? 0,
      (n, was) => {
        if (was === undefined || n <= was) return;
        setRetry({ retries: n, at: Date.now() });
        // The hold ends: read the rule again.
        const t = setTimeout(() => setHeld((h) => h + 1), TRYING_AGAIN_MS);
        onCleanup(() => clearTimeout(t));
      },
    ),
  );
  createEffect(() => {
    if (snap()?.phase === "ready") setRetry(null);
  });
  const again = () => {
    held();
    return tryingAgain(snap()?.phase, retry(), Date.now());
  };
  const url = () => (snap()?.phase === "ready" ? snap()?.url : null) ?? null;
  // The version that plays: the core names it from `ready` on; before that, the one this play
  // opened (so the loading screen can say "Starting 1080p, 1.5 GB from Torrentio").
  const playing = () => [...(versions()?.recommended ? [versions()?.recommended] : []), ...(versions()?.rows ?? [])].find((r) => r?.key === (snap()?.key ?? key())) ?? null;

  const [osd, setOsd] = createSignal(true);
  const [panel, setPanel] = createSignal<"subs" | "audio" | null>(null);
  // The language whose options the subtitle picker shows (its second level off the TV).
  const [subLanguage, setSubLanguage] = createSignal<string | null>(null);
  createEffect(on(panel, (p) => p !== "subs" && setSubLanguage(null)));
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
    onLoaded: () => {
      sendDuration();
      // Started over: from now on this entry resumes (a reload, Forward).
      if (props.startOver && (history.state as { startOver?: boolean } | null)?.startOver)
        history.replaceState({ ...(history.state as object), startOver: undefined }, "");
    },
    onSubtitleError: (lang) => setSubError(`Could not load ${languageName(lang)} subtitles from the core`),
  });
  const { time, duration, paused, buffered, started, waiting, rate } = engine;
  // Subtitle timing per track (the size is the profile's, synced): a core
  // track starts at the delay the core keeps for it (`delayMs`), and a change
  // goes back to the core once the stepper settles and when the play closes.
  const [delays, setDelays] = createSignal<Record<string, number>>({});
  const subDelay = () => {
    const url = track()?.url;
    if (!url) return 0;
    return delays()[url] ?? (snap()?.subtitles.find((t) => t.url === url)?.delayMs ?? 0) / 1000;
  };
  createEffect(() => engine.setSubtitleDelay(subDelay()));
  let unsent: { url: string; ms: number } | null = null;
  let settle: ReturnType<typeof setTimeout> | undefined;
  const sendDelay = () => {
    clearTimeout(settle);
    if (unsent) void playback()?.command({ type: "subtitleDelay", ...unsent });
    unsent = null;
  };
  const setSubDelay = (sec: number) => {
    const url = untrack(track)?.url;
    if (!url) return;
    // Another track's change still waiting goes first.
    if (unsent && unsent.url !== url) sendDelay();
    setDelays({ ...delays(), [url]: sec });
    unsent = { url, ms: Math.round(sec * 1000) };
    clearTimeout(settle);
    settle = setTimeout(sendDelay, DELAY_SETTLE_MS);
  };
  // A desktop has no volume keys of its own here: a mute button and a level,
  // and full screen (with Back in the header on a desktop and a phone). The
  // level and mute carry on to the next episode (`sound`); full screen stays
  // through the next episode or version and ends with the play (app.tsx).
  const [volume, setVolumeLevel] = createSignal(sound.volume);
  const [muted, setMuted] = createSignal(sound.muted);
  createEffect(() => {
    sound.volume = volume();
    sound.muted = muted();
    engine.setVolume(muted() ? 0 : volume());
  });
  // A phone draws its lines itself (the same rule on the iPhone and
  // Android), so they are placed exactly: under the picture at full size
  // when the black band below it holds them (portrait); else at the
  // picture's foot, inside it, and while the controls show, 8 px over the
  // scrubber where they cover the picture (landscape). Lines that would
  // still meet the transport shrink to 85% while the controls show. Never
  // under a control, never at the top.
  const ownCues = () => form() === "phone";
  createEffect(() => engine.setSubtitleOwn(ownCues()));
  const [lines, setLines] = createSignal<string[]>([]);
  const [shrink, setShrink] = createSignal(false);
  // Until placed, at the foot (never a frame at the top).
  const [cueAt, setCueAt] = createSignal<{ top?: number; bottom?: number }>({ bottom: 16 });
  let cuesEl: HTMLDivElement | undefined;
  const readCues = () => {
    const video = engine.view as HTMLVideoElement;
    const active = track() ? video.textTracks[0]?.activeCues : null;
    setLines(active ? Array.from(active).flatMap((c) => (c as VTTCue).text.replace(/<[^>]+>/g, "").split("\n")) : []);
  };
  const placeCues = () => {
    const video = engine.view as HTMLVideoElement;
    if (!ownCues() || !cuesEl) return;
    const box = video.getBoundingClientRect();
    // The picture inside the element (object-fit: contain).
    const scale = video.videoWidth && video.videoHeight ? Math.min(box.width / video.videoWidth, box.height / video.videoHeight) : 0;
    const band = scale ? (box.height - video.videoHeight * scale) / 2 : 0;
    const pictureFoot = box.bottom - band;
    const bar = document.querySelector<HTMLElement>(".player-scr .osd")?.getBoundingClientRect();
    const scrubber = document.querySelector<HTMLElement>(".player-scr .scrub")?.getBoundingClientRect();
    const transport = document.querySelector<HTMLElement>(".player-scr .transport")?.getBoundingClientRect();
    const up = osd() && !ended() && !!bar;
    // The lines' height at full size (drawn now at 85% when shrunk).
    const height = cuesEl.getBoundingClientRect().height / (shrink() ? 0.85 : 1);
    if (band > 0 && (bar?.top ?? box.bottom) - pictureFoot >= height + 8) {
      setShrink(false);
      return setCueAt({ top: pictureFoot - box.top + 10 });
    }
    let foot = band > 0 ? pictureFoot - 8 : box.bottom - 16;
    if (up && scrubber) foot = Math.min(foot, scrubber.top - 8);
    setShrink(up && !!transport && foot - height < transport.bottom && foot > transport.top);
    setCueAt({ bottom: box.bottom - foot });
  };
  createEffect(on([lines, osd, ended, () => prefs.state.data?.subtitleSize], () => requestAnimationFrame(placeCues)));
  onMount(() => {
    const video = engine.view as HTMLVideoElement;
    window.addEventListener("resize", placeCues);
    video.addEventListener("loadedmetadata", placeCues);
    video.addEventListener("resize", placeCues);
    // Non-bubbling, from the track element: caught on the way down.
    video.addEventListener("cuechange", readCues, true);
    onCleanup(() => {
      window.removeEventListener("resize", placeCues);
      video.removeEventListener("loadedmetadata", placeCues);
      video.removeEventListener("resize", placeCues);
      video.removeEventListener("cuechange", readCues, true);
    });
  });
  const [full, setFull] = createSignal(!!document.fullscreenElement);
  onMount(() => {
    const changed = () => setFull(!!document.fullscreenElement);
    document.addEventListener("fullscreenchange", changed);
    onCleanup(() => document.removeEventListener("fullscreenchange", changed));
  });
  const toggleFull = () => void (document.fullscreenElement ? document.exitFullscreen() : document.documentElement.requestFullscreen()).catch(() => undefined);
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
      // Versions over the player owns the keys; its Back closes it.
      if (layer()) {
        if (isBackKey(e)) {
          e.preventDefault();
          closeLayer();
        }
        return;
      }
      // Backspace in the subtitle search deletes a letter.
      const typing = e.target instanceof HTMLInputElement && e.target.type !== "range";
      const back = e.key === "Escape" || (e.key === "Backspace" && !typing) || e.key === "GoBack" || e.keyCode === 10009 || e.keyCode === 461;
      if (e.key === "MediaPlayPause" || e.key === "MediaPlay" || e.key === "MediaPause") {
        e.preventDefault();
        toggle();
        return;
      }
      // Back in a language's options steps back to the languages (off the TV).
      if (back && panel() === "subs" && subLanguage() && form() !== "tv") {
        e.preventDefault();
        e.stopImmediatePropagation();
        const was = subLanguage();
        setSubLanguage(null);
        autoFocusSoon(() => document.querySelector<HTMLElement>(`[data-f="sublang:${was}"]`));
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
      if (!panel() && !(e.target as Element | null)?.closest?.("input, textarea, select")) {
        // Space and K play or pause from anywhere; Space on a focused control
        // with the controls up stays that control's own press.
        const onControl = (document.activeElement as Element | null)?.closest("button, a, [role=slider], input");
        if (e.key === "k" || e.key === "K" || (e.key === " " && (!osd() || !onControl))) {
          e.preventDefault();
          e.stopImmediatePropagation();
          toggle();
          return;
        }
        // F, as on YouTube and Netflix: full screen in and out (a desktop).
        if ((e.key === "f" || e.key === "F") && form() === "desktop") {
          e.preventDefault();
          e.stopImmediatePropagation();
          toggleFull();
          return;
        }
        // A stalled frame offering Choose another version: the first arrow shows the controls with it focused.
        if (!osd() && wait() === "stall" && another() && e.key.startsWith("Arrow")) {
          e.preventDefault();
          e.stopImmediatePropagation();
          wake();
          document.querySelector<HTMLElement>('[data-f="player:another"]')?.focus();
          return;
        }
        // Controls hidden: Left and Right skip 10 s and bring the controls up,
        // as a click on the Apple TV remote's edge does.
        if (!osd() && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
          e.preventDefault();
          e.stopImmediatePropagation();
          seek(e.key === "ArrowLeft" ? -10 : 10);
          return;
        }
      }
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
      // The last progress and delay go before the view closes; anything the
      // page sends after (the video's own pause as it leaves) is a no-op.
      report();
      sendDelay();
      void playback()?.close();
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
      // Ready under Versions: it loads held there, so nothing plays or sounds until the viewer plays it.
      engine.load(u, prev ? engine.position() : start.next(props.startOver ? 0 : (snap()?.resumeMs ?? 0) / 1000), untrack(layer));
    }),
  );

  const toggle = () => {
    engine.toggle();
    wake();
  };
  // Whether the pointer press under way only woke the controls.
  let woke = false;
  // A click on a control or a card, not on the picture.
  const onControls = (e: MouseEvent) => !!(e.target as Element).closest("button, a, input, [role=slider], aside, .endcard, .skip-wrap, .next, .osd");
  const seek = (delta: number) => {
    if (!duration()) return;
    engine.seek(Math.min(Math.max(0, engine.position() + delta), duration() - 0.5));
    wake();
  };
  const seekAt = (e: PointerEvent & { currentTarget: HTMLElement }) => {
    const r = e.currentTarget.getBoundingClientRect();
    const at = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
    engine.seek(Math.min(at * duration(), duration() - 0.5));
    wake();
  };
  const cycleSpeed = () => engine.setRate(SPEEDS[(SPEEDS.indexOf(rate()) + 1) % SPEEDS.length] ?? 1);

  // The core serves every subtitle as UTF-8 WebVTT on a loopback URL.
  const showSubtitle = (t: SubtitleTrack | null) => {
    setSubError(null);
    const sub = t ? { url: t.url, lang: t.lang } : null;
    setTrack(sub);
    engine.setSubtitle(sub);
    if (!sub) setLines([]);
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
  // Notes already said, for the playing source (they clear with it).
  let notesSeen = 0;
  // The core's start pick (`autoSubtitle`, plans/m13-subtitle-languages.md
  // 6): turned on once the list settles, unless the viewer chose already.
  // A new version (the core moved on) first drops the old one's subtitle,
  // which went with its source, so the new one's start pick applies; the
  // core keeps the viewer's own choice for the title and carries it over.
  // One effect, so the drop always comes before the pick.
  let version: string | null = null;
  createEffect(() => {
    const s = snap();
    const k = s?.key ?? null;
    if (k && version && k !== version) {
      chose = false;
      notesSeen = 0;
      setSubError(null);
      setTrack(null);
      engine.setSubtitle(null);
    }
    if (k) version = k;
    const auto = s?.autoSubtitle;
    if (!auto || chose) return;
    // Once the file has opened, its own track in that language (not forced)
    // wins over the core's, even one already on (plans/player-polish.md 2.2).
    const files = engine.tracks().filter((t) => t.kind === "sub");
    const pick = startOption(pickerLanguages(s?.subtitles ?? [], files, languageName), auto);
    if (!pick) return;
    if ("file" in pick) {
      if (pick.file.selected) return;
      if (untrack(track)) showSubtitle(null);
      engine.selectTrack("sub", pick.file.id);
    } else if (!untrack(track)) showSubtitle(pick.track);
  });

  // A file the core could not serve plays as an empty track and comes back
  // as a note. Only the picked track is fetched, so a new note is about it:
  // say so, and turn it off when it left the list.
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
    // A panel open (Subtitles during the credits): Up Next waits, as Skip does.
    if (!next() || duration() <= 60 || ended() || panel()) return false;
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
          autoFocusSoon(() => document.querySelector<HTMLElement>('[data-f="player:skip"]'));
        } else if (!now && was && !document.activeElement?.closest(".player-scr [data-f]")) {
          autoFocusSoon(() => document.querySelector<HTMLElement>('[data-f="player:toggle"]'));
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
  // Another version of this video (the failure card's offer): a new play at
  // its own address, on the same Versions view (same binge group).
  const playKey = (key: string) =>
    navigate(`${playPath(props.kind, props.id, props.videoId)}${props.binge ? `?binge=${encodeURIComponent(props.binge)}` : ""}`, { replace: true, state: { ...playState(key), ...carried() } });
  // The layer opens on its Play; closed, it hands its choice to the player, else the focus comes back here.
  createEffect(
    on(
      layer,
      (open) => {
        const choice = picked;
        picked = null;
        if (open) return restoreSoon(false);
        // The app's Back marked a Back for a new screen; the path did not change.
        takeBack();
        if (choice) return playKey(choice);
        if (resume) engine.play();
        resume = false;
        autoFocusSoon(() => document.querySelector<HTMLElement>('[data-f="player:another"]:not([inert])') ?? document.querySelector<HTMLElement>('[data-f="player:toggle"]'));
      },
      { defer: true },
    ),
  );
  // Up Next takes focus when it appears, like Skip buttons on other players.
  createEffect(
    on(showNext, (shown, was) => {
      if (shown && !was) {
        setOsd(true);
        autoFocusSoon(() => document.querySelector<HTMLElement>('[data-f="next:play"]'));
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
      if (target) autoFocusSoon(() => target);
    }),
  );

  // The end of the file: an end card with the next step, never a dead end.
  const onEnded = () => {
    report();
    const href = nextHref();
    if (href && prefs.state.data?.autoplayNext) navigate(href, { replace: true, state: carried() });
    else {
      setOsd(false);
      setEnded(true);
    }
  };
  createEffect(
    on(ended, (e) => {
      if (e) autoFocusSoon(() => document.querySelector<HTMLElement>(".endcard [data-autofocus]"));
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
    // A track in the file is the viewer's choice too, and its language the profile's.
    chose = true;
    void playback()?.command(fileChoice(fileSubs().find((t) => t.id === id)?.lang ?? null));
    pickSubtitle(null);
    engine.selectTrack("sub", id);
  };

  // With no track on: the languages the picker shows so far, and whether the core still asks.
  const subsOff = () => offSummary(pickerLanguages(snap()?.subtitles ?? [], fileSubs(), languageName).length, !!snap()?.subtitlesPending);
  // The Subtitles button's value: the language on, else the count.
  const subsText = () => {
    const on = track() ? snap()?.subtitles.find((t) => t.url === track()?.url) : undefined;
    if (track()) return on?.languageName ?? languageName(track()?.lang ?? "");
    const file = fileSubs().find((t) => t.selected);
    return file?.lang ? languageName(file.lang) : subsOff().text;
  };
  const sizeAt = () => Math.max(0, SIZES.findIndex((s) => s.size === (prefs.state.data?.subtitleSize ?? "medium")));
  const setSize = (i: number) => {
    const s = SIZES[i];
    if (s) void op("setPrefs", { patch: { subtitleSize: s.size } }).catch(() => undefined);
  };
  const pct = (x: number) => `${duration() ? Math.min(100, (x / duration()) * 100) : 0}%`;
  // The failure card's way to Versions (the plan's shared Step 3 table): "Choose another version"
  // after a failure, "Choose a version" when none plays here; primary and focused when nothing else is.
  const lone = (e?: EmptyVersions) => !!e && !emptyWords(e).steps.length;
  const chooseVersion = () => (
    <A
      class="btn big"
      classList={{ primary: !snap()?.offer && !failed() }}
      href={versionsPath(props.kind, props.id, props.videoId)}
      replace
      on:click={toVersions}
      data-f="player:versions"
      data-autofocus={snap()?.offer || failed() ? undefined : ""}
    >
      <Layers aria-hidden />
      {failed() || openError() ? nextVersion.another : choiceWords.empty.nothingPlaysHere.action}
    </A>
  );
  const status = (): { tone: string; text: string; detail?: string; note?: string; busy: boolean; empty?: EmptyVersions } | null => {
    const t = titleError();
    if (t) return { tone: "bad", text: nextVersion.titleNotOpened, detail: t, busy: false };
    const v = ctx.versions();
    if (v?.state.status.state === "failed") return { tone: "bad", text: nextVersion.listNotLoaded, detail: (v.state.status as { reason: string }).reason, busy: false };
    // The playback could not open (a version no longer listed): a card with
    // the error under it, as Apple's, never a spinner.
    const open = openError();
    if (open) return { tone: "bad", text: nextVersion.notOpened, detail: open, busy: false };
    const s = snap();
    if (s?.phase === "ready") return null;
    // A Try again that failed again at once keeps the card, with "Trying
    // again" under its headline for the hold, as Android and Apple do.
    // Why nothing played, in the shared words when the core names it ("This didn't start"); a hand pick
    // with an offer says so.
    if (s?.phase === "failed") return { tone: "bad", text: s.empty && !s.offer ? emptyWords({ kind: s.empty, action: "none" }).text : nextVersion.failed(!!s.offer), note: again() ? nextVersion.tryingAgain : undefined, busy: false };
    if (s?.phase === "resolving") {
      // Moving on (the recommendation failed for a reason of its own), or Try again.
      const restart = restartLine(s, again());
      if (restart) return { tone: "warn", text: restart, busy: true };
      return { tone: "warn", text: startLine(), busy: true };
    }
    // A version picked by hand that Versions has not listed yet.
    if (props.chosen && !key()) return { tone: "warn", text: nextVersion.finding, busy: true };
    const vs = versions();
    // Why nothing can play, with its actions (the shared Step 3 table).
    if (vs?.empty) return { tone: "bad", text: emptyWords(vs.empty).text, busy: false, empty: vs.empty };
    // Never an addon's name while it waits.
    return { tone: "warn", text: ctx.details() ? nextVersion.findingBest : "Opening", busy: true };
  };
  // The pick as it starts: "Starting 1080p, 1.5 GB" (no addon's name).
  const startLine = () => {
    const r = playing();
    return r ? `Starting ${qualityText(r.quality)}${r.sizeBytes ? `, ${bytes(r.sizeBytes)}` : ""}` : "Starting";
  };

  // The loading layer (plans/player-polish.md 1.2): while there is no
  // picture yet, and on a stall after the first frame (the element waits for
  // data while playing; a seek's short wait does not count).
  const [stalled, setStalled] = createSignal(false);
  createEffect(() => {
    if (!started() || !waiting() || paused() || ended()) return setStalled(false);
    const t = setTimeout(() => setStalled(true), STALL_MS);
    onCleanup(() => clearTimeout(t));
  });
  const wait = createMemo((): Wait | null => {
    if (ended()) return null;
    const s = status();
    if (s) return s.busy ? "start" : null;
    // The URL is in, the first frame is not, and nothing paused it (a
    // browser that refuses to play on its own shows the controls instead).
    if (url() && !started() && !paused()) return "start";
    return stalled() ? "stall" : null;
  });
  // The core's progress until the player has the URL (it keeps the last it
  // sent once `start` clears at `ready`), then the player's own buffer fills
  // the rest; a stall fills from the buffer alone.
  const [coreFill, setCoreFill] = createSignal(0);
  createEffect(() => {
    const p = snap()?.start?.progress;
    if (p != null) setCoreFill(p);
  });
  const fill = () => {
    if (!url()) return startFill(coreFill(), 0);
    if (wait() === "stall") return startFill(0, bufferShare(buffered(), engine.position(), STALL_AHEAD_S));
    return startFill(coreFill(), bufferShare(buffered(), engine.position()));
  };
  const waitLines = () => (wait() === "stall" ? [] : loadingLines(status()?.text ?? startLine(), snap()?.start?.caption));
  // The core's "Choose another version" (plans/play-every-version.md 2e): 20 s into the start, or over a
  // stalled frame it may not move on from; under the stage line, laid out from the start but hidden and
  // inert, so nothing moves and nothing can focus it; never focused by itself; said once when it appears.
  const another = () => !!snap()?.chooseAnother && (wait() === "start" || wait() === "stall");
  // A wait for data mid-play, told to the core as it starts and ends (the core judges seeks and pauses).
  const midStall = createMemo(() => started() && waiting() && !paused() && !ended());
  createEffect(
    on(midStall, (stalled) => void playback()?.command({ type: "stalled", stalled, positionMs: Math.round(engine.position() * 1000) }), { defer: true }),
  );

  // Laid out from the start, hidden and inert until the offer, so nothing moves and nothing focuses it.
  const anotherButton = () => (
    <A class="btn ld-another" classList={{ shown: another() }} href={layerHref()} on:click={openLayer} data-f="player:another" inert={!another()}>
      <Layers aria-hidden />
      {nextVersion.another}
    </A>
  );
  return (
    <>
    <div
      class="player-scr"
      inert={layer() || undefined}
      data-scheme="dark"
      data-subsize={prefs.state.data?.subtitleSize ?? "medium"}
      // Not idle while the loading screen waits: Back, its one control, stays.
      classList={{ idle: !osd() && !!url() && wait() !== "start", started: started(), paused: pausedNow(), loading: wait() === "start", stalling: wait() === "stall", offering: another() }}
      onPointerMove={wake}
      onPointerDown={(e) => {
        // While the controls are hidden, a click only brings them back.
        woke = !osd();
        if (woke) {
          e.preventDefault();
          wake();
        }
      }}
      onClick={(e) => {
        // With the controls up, a click on the picture plays or pauses (a
        // mouse; a touch screen shows and hides the controls).
        if (woke || form() !== "desktop" || !started()) return;
        if (onControls(e)) return;
        toggle();
      }}
      // A double click on the picture: full screen in and out (its two
      // clicks played and paused, so the play goes on as it was).
      onDblClick={(e) => {
        if (form() !== "desktop" || onControls(e)) return;
        toggleFull();
      }}
    >
      <Backdrop src={ctx.details()?.background} class="player-bd" dim />
      {engine.view}
      {/* Not over an open panel or a card. */}
      <Show when={ownCues() && lines().length > 0 && !panel() && !ended()}>
        <div
          class="cues"
          classList={{ small: shrink() }}
          ref={cuesEl}
          aria-live="off"
          style={{ top: cueAt().top !== undefined ? `${cueAt().top}px` : undefined, bottom: cueAt().bottom !== undefined ? `${cueAt().bottom}px` : undefined }}
        >
          <For each={lines()}>{(l) => <span>{l}</span>}</For>
        </div>
      </Show>
      {/* Paused, the picture steps back and the title says what it is, as
          the Apple TV app, Netflix and Infuse do. */}
      <div class="pdim" aria-hidden="true" />
      <Loading
        wait={wait()}
        fill={fill()}
        logo={ctx.details()?.logo}
        name={ctx.details()?.name}
        background={ctx.details()?.background}
        lines={waitLines()}
        action={form() === "tv" ? anotherButton() : undefined}
      />
      <p class="sr" aria-live="polite">
        {another() ? nextVersion.anotherReady : ""}
      </p>
      <div class="osd-top" />
      <div class="osd-bot" />

      <header class="ptitle">
        {/* A remote has Back; a desktop and a phone have it here, in every state. */}
        <Show when={form() !== "tv"}>
          {/* Choose another version sits beside Back here (under the stage line on TV): Tab reaches it after Back. */}
          <div class="pback-row">
            <button class="pback" data-f="player:leave" aria-label="Back" onClick={goBack}>
              <ArrowLeft aria-hidden />
            </button>
            {anotherButton()}
          </div>
        </Show>
        <h1>{ctx.details()?.name ?? ""}</h1>
        <Show when={ctx.label()}>{(l) => <p>{l()}</p>}</Show>
        <Show when={pausedNow() && (ctx.episode()?.overview ?? ctx.details()?.description)}>{(t) => <p class="psyn">{t()}</p>}</Show>
        <TraktMark state={snap()?.trakt} />
      </header>
      <Show when={endsAt()}>{(t) => <p class="clock num">Ends at {t()}</p>}</Show>

      {/* Still on its way: the loading layer says so (above). */}
      <Show when={status()}>
        {(s) => (
          <Show when={!s().busy}>
            {/* Nothing plays: a centered card with the way on, never a
                black screen with a line of text. */}
            <section class="endcard" role="alert">
              <div class="end-body">
                <p class="st bad">
                  <CircleAlert aria-hidden />
                  {s().text}
                </p>
                <Show when={s().detail}>{(d) => <Why class="end-detail" reason={d()} />}</Show>
                <Show when={s().note}>
                  {(n) => (
                    <p class="end-detail end-note" role="status">
                      <LoaderCircle aria-hidden class="spin" />
                      {n()}
                    </p>
                  )}
                </Show>
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
                  {/* A version picked by hand failed for a reason of its
                      own: the ranking's next one, first and focused. */}
                  <Show when={snap()?.offer}>
                    {(o) => (
                      <button
                        class="btn primary big"
                        data-f="player:offer"
                        data-autofocus
                        onClick={() => playKey(o().key)}
                      >
                        <Play aria-hidden />
                        {nextVersion.play(o().label)}
                      </button>
                    )}
                  </Show>
                  {/* A playback that could not open: mostly a version no
                      longer listed, which Try again cannot bring back, so
                      Choose a version comes first and has the focus. */}
                  <Show when={openError() && !titleError()}>{chooseVersion()}</Show>
                  <Show when={s().empty}>
                    {(e) => <EmptyActs empty={e()} at="player" big retry={props.retry} play={playKey} choose={chooseVersion()} />}
                  </Show>
                  {/* This play again from its first pick (`retry`), or the
                      playback opened again: what failed may be the moment. */}
                  <Show when={(failed() || openError() || titleError()) && failedAs() !== "noRoom" && failedAs() !== "nothingPlaysHere"}>
                    <button
                      class="btn big"
                      classList={{ primary: !snap()?.offer && (!openError() || !!titleError()) }}
                      data-f="player:retry"
                      data-autofocus={snap()?.offer || (openError() && !titleError()) ? undefined : ""}
                      onClick={() => (titleError() ? props.retry() : openError() ? setOpens((n) => n + 1) : void playback()?.command({ type: "retry" }))}
                    >
                      <RefreshCw aria-hidden />
                      Try again
                    </button>
                  </Show>
                  <Show when={!openError() && !titleError() && !s().empty && failedAs() !== "noRoom" && failedAs() !== "unreachable"}>{chooseVersion()}</Show>
                  {/* The one way on when nothing plays and the state has no action of its own. */}
                  <button
                    class="btn big"
                    classList={{ primary: lone(s().empty) }}
                    data-f="player:back"
                    data-autofocus={lone(s().empty) ? "" : undefined}
                    onClick={goBack}
                  >
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
                    {/* Back goes where the viewer came from: it names the title
                        only when that is the title page. */}
                    <button class="btn primary big" data-f="end:back" data-autofocus onClick={goBack}>
                      <ArrowLeft aria-hidden />
                      {backWords()}
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
                    <A class="btn primary big" href={nextHref() ?? "#"} replace state={carried()} data-f="end:next" data-autofocus>
                      <SkipForward aria-hidden />
                      Next episode
                    </A>
                    <button class="btn big" data-f="end:again" onClick={playAgain}>
                      <RotateCcw aria-hidden />
                      Play again
                    </button>
                    <button class="btn big" data-f="end:back" onClick={goBack}>
                      <ArrowLeft aria-hidden />
                      {backWords()}
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

      {/* Not before the first frame: the loading screen has nothing to act on
          but Back (and on TV no focus at all). */}
      <Show when={url() && !ended() && wait() !== "start"}>
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
              // A press seeks there and a drag follows the pointer (mouse and touch).
              onPointerDown={(e) => {
                if (e.button !== 0 || !duration()) return;
                e.currentTarget.setPointerCapture(e.pointerId);
                seekAt(e);
              }}
              onPointerMove={(e) => e.currentTarget.hasPointerCapture(e.pointerId) && seekAt(e)}
            >
              <For each={buffered()}>{([a, b]) => <i class="buf" style={{ left: pct(a), width: `calc(${pct(b)} - ${pct(a)})` }} />}</For>
              <i class="pos" style={{ width: pct(time()) }} />
              <b class="knob" style={{ left: pct(time()) }} />
            </div>
            <span class="num">-{clock(remaining())}</span>
          </div>
          <div class="ctl">
            <Show when={form() === "desktop"}>
              <div class="pdesk">
                <button class="picon" data-f="player:mute" aria-label={muted() ? "Unmute" : "Mute"} aria-pressed={muted()} onClick={() => setMuted(!muted())}>
                  <Show when={muted() || volume() === 0} fallback={<Volume2 aria-hidden />}>
                    <VolumeX aria-hidden />
                  </Show>
                </button>
                <input
                  class="pvol"
                  type="range"
                  min="0"
                  max="100"
                  step="5"
                  value={volume()}
                  aria-label="Volume"
                  data-f="player:volume"
                  onInput={(e) => {
                    setVolumeLevel(Number(e.currentTarget.value));
                    setMuted(false);
                  }}
                />
                <button class="picon" data-f="player:fullscreen" aria-label={full() ? "Exit full screen" : "Full screen"} onClick={toggleFull}>
                  <Show when={full()} fallback={<Maximize aria-hidden />}>
                    <Minimize aria-hidden />
                  </Show>
                </button>
              </div>
            </Show>
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
                  {subsText()}
                  <Show when={!track() && subsOff().searching}>
                    <LoaderCircle class="spin mark" role="img" aria-label="Still searching" />
                  </Show>
                </small>
              </button>
              {/* Leaving for Versions or Episodes replaces this play in history:
                  Back from the next play goes to Versions, then the title, never to this one. */}
              <A
                class="pa"
                href={versionsPath(props.kind, props.id, props.videoId)}
                replace
                on:click={toVersions}
                data-f="player:versionsact"
              >
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
                  <A class="pa" href={titlePath(props.kind, props.id)} replace on:click={toTitle} data-f="player:episodes">
                    <ListVideo aria-hidden />
                    <b>Episodes</b>
                    <small>{r().season === null ? "" : seasonName(r().season as number)}</small>
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
            <A
              class="opt"
              href={versionsPath(props.kind, props.id, props.videoId)}
              replace
              on:click={toVersions}
              data-f="audio:versions"
            >
              <b>Choose a version</b>
              <span>{versions()?.rows.length ? `${versions()?.rows.length} more listed` : "See what is listed"}</span>
            </A>
          </div>
        </aside>
      </Show>

      <Show when={panel() === "subs"}>
        <SubtitlePicker
          tracks={snap()?.subtitles ?? []}
          fileSubs={fileSubs()}
          on={track()?.url ?? null}
          onFile={track() ? null : (fileSubs().find((t) => t.selected)?.id ?? null)}
          pending={!!snap()?.subtitlesPending}
          notes={(snap()?.subtitleNotes ?? []).map(subtitleNote)}
          open={subLanguage()}
          setOpen={setSubLanguage}
          timing={track() ? { value: subDelay(), set: setSubDelay, track: track()?.url ?? "" } : null}
          size={{
            text: SIZES[sizeAt()]?.text ?? "100%",
            less: sizeAt() > 0 ? () => setSize(sizeAt() - 1) : undefined,
            more: sizeAt() < SIZES.length - 1 ? () => setSize(sizeAt() + 1) : undefined,
          }}
          pick={chooseSubtitle}
          pickFile={pickFileSub}
          close={() => setPanel(null)}
        />
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
                  <p class="num">{startsText(remaining())}</p>
                </Match>
              </Switch>
              <A class="btn primary" href={nextHref() ?? "#"} replace state={carried()} data-f="next:play">
                <SkipForward aria-hidden />
                Play now
              </A>
            </div>
          </aside>
        )}
      </Show>
    </div>
      <Show when={layer()}>
        <div class="vlayer">
          <VersionsPage kind={props.kind} id={props.id} videoId={props.videoId} binge={props.binge} retry={props.retry} onPlay={pickInLayer} onBack={closeLayer} hold />
        </div>
      </Show>
    </>
  );
}
