import { A, useNavigate, useParams, useSearchParams } from "@solidjs/router";
import Captions from "lucide-solid/icons/captions";
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
import { liveView, prefsView } from "../core/client";
import type { PlaybackSnapshot } from "../core/generated/PlaybackSnapshot";
import type { SubtitleTrack } from "../core/generated/SubtitleTrack";
import { videoContext } from "../core/video";
import { focusEl } from "../focus/dom";
import { bytes, clock, decoded, episodeLabel, playPath, seasonName, titlePath, versionsPath } from "../lib/format";
import { toVtt } from "../lib/subs";
import { languageName } from "../lib/words";
import "./player.css";

const SPEEDS = [1, 1.25, 1.5, 2, 0.75];
const HIDE_MS = 4000;
const PROGRESS_MS = 5000;
// Up Next appears this many seconds before the end.
const NEXT_AT = 20;

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
  let video!: HTMLVideoElement;

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

  // Media state mirrored from the element.
  const [paused, setPaused] = createSignal(true);
  const [started, setStarted] = createSignal(false);
  const [time, setTime] = createSignal(0);
  const [duration, setDuration] = createSignal(0);
  const [buffered, setBuffered] = createSignal<[number, number][]>([]);
  const [rate, setRate] = createSignal(1);
  const [osd, setOsd] = createSignal(true);
  const [panel, setPanel] = createSignal<"subs" | null>(null);
  const [track, setTrack] = createSignal<{ src: string; lang: string } | null>(null);
  const [subError, setSubError] = createSignal<string | null>(null);
  let resumed = false;
  let hideTimer: ReturnType<typeof setTimeout> | undefined;

  const wake = () => {
    setOsd(true);
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      if (!video?.paused && !panel()) setOsd(false);
    }, HIDE_MS);
  };

  const report = () => {
    const p = playback();
    if (!p || !video || !Number.isFinite(video.duration) || video.duration <= 0) return;
    void p.command({ type: "progress", positionMs: Math.round(video.currentTime * 1000), durationMs: Math.round(video.duration * 1000) });
  };

  onMount(() => {
    wake();
    const timer = setInterval(() => !video?.paused && report(), PROGRESS_MS);
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
      if (!osd() && !back) {
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
      const t = track();
      if (t) URL.revokeObjectURL(t.src);
    });
  });

  // A new source (first start or failover) keeps the position.
  createEffect(
    on(url, (u, prev) => {
      if (!u || !video) return;
      const at = prev ? video.currentTime : (snap()?.resumeMs ?? 0) / 1000;
      video.src = u;
      resumed = false;
      video.addEventListener(
        "loadedmetadata",
        () => {
          if (!resumed && at > 0) video.currentTime = at;
          resumed = true;
        },
        { once: true },
      );
      void video.play().catch(() => setPaused(true));
    }),
  );

  const toggle = () => {
    if (!video) return;
    if (video.paused) void video.play().catch(() => undefined);
    else video.pause();
    wake();
  };
  const seek = (delta: number) => {
    if (!video || !Number.isFinite(video.duration)) return;
    video.currentTime = Math.min(Math.max(0, video.currentTime + delta), video.duration - 0.5);
    wake();
  };
  const cycleSpeed = () => {
    const next = SPEEDS[(SPEEDS.indexOf(rate()) + 1) % SPEEDS.length] ?? 1;
    video.playbackRate = next;
    setRate(next);
  };

  const pickSubtitle = async (t: SubtitleTrack | null) => {
    setPanel(null);
    setSubError(null);
    const old = track();
    if (old) URL.revokeObjectURL(old.src);
    setTrack(null);
    if (!t) return;
    try {
      const res = await fetch(t.url);
      if (!res.ok) throw new Error(`the subtitle server answered ${res.status}`);
      const blob = new Blob([toVtt(await res.text())], { type: "text/vtt" });
      setTrack({ src: URL.createObjectURL(blob), lang: t.lang });
      queueMicrotask(() => {
        const tt = video.textTracks[0];
        if (tt) tt.mode = "showing";
      });
    } catch (e) {
      setSubError(`Could not load ${languageName(t.lang)} subtitles: ${e instanceof Error ? e.message : e}`);
    }
  };

  const onError = () => {
    const err = video.error;
    const why =
      err?.code === MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED
        ? "this device cannot decode the file"
        : err?.code === MediaError.MEDIA_ERR_NETWORK
          ? "the network dropped"
          : (err?.message ?? "player error");
    void playback()?.command({ type: "failOver", error: why });
  };

  const remaining = () => Math.max(0, duration() - time());
  const endsAt = () => {
    if (!duration()) return null;
    const at = new Date(Date.now() + (remaining() / rate()) * 1000);
    return at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  };
  const next = () => snap()?.next ?? null;
  const showNext = () => !!next() && duration() > 60 && remaining() <= NEXT_AT;
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
        queueMicrotask(() => {
          const el = document.querySelector<HTMLElement>('[data-f="next:play"]');
          if (el) focusEl(el);
        });
      }
    }),
  );
  const onEnded = () => {
    report();
    const href = nextHref();
    if (href && prefs.state.data?.autoplayNext) navigate(href, { replace: true });
    else setOsd(true);
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
      return { tone: "warn", text: r ? `Starting ${r.quality}${r.sizeBytes ? `, ${bytes(r.sizeBytes)}` : ""} from ${r.via}` : "Starting", busy: true };
    }
    const vs = versions();
    if (vs?.done && !vs.recommended && !vs.rows.length) return { tone: "bad", text: "No playable version found", busy: false };
    if (vs?.pendingAddons.length) return { tone: "warn", text: `Asking ${vs.pendingAddons.join(", ")} for versions`, busy: true };
    return { tone: "warn", text: ctx.details() ? "Choosing the best version" : "Opening", busy: true };
  };

  return (
    <div class="player-scr" classList={{ idle: !osd() && !!url() }} onPointerMove={wake}>
      <Backdrop src={ctx.details()?.background} class="player-bd" dim />
      <video
        ref={video}
        class="video"
        classList={{ on: started() }}
        playsinline
        preload="auto"
        onPlay={() => {
          setPaused(false);
          wake();
        }}
        onPlaying={() => setStarted(true)}
        onPause={() => {
          setPaused(true);
          setOsd(true);
          report();
        }}
        onTimeUpdate={() => setTime(video.currentTime)}
        onDurationChange={() => setDuration(Number.isFinite(video.duration) ? video.duration : 0)}
        onProgress={() => {
          const out: [number, number][] = [];
          for (let i = 0; i < video.buffered.length; i++) out.push([video.buffered.start(i), video.buffered.end(i)]);
          setBuffered(out);
        }}
        onEnded={onEnded}
        onError={onError}
      >
        <Show when={track()}>{(t) => <track kind="subtitles" src={t().src} srclang={t().lang} default />}</Show>
      </video>
      <div class="osd-top" />
      <div class="osd-bot" />

      <header class="ptitle">
        <h1>{ctx.details()?.name ?? ""}</h1>
        <Show when={ctx.label()}>{(l) => <p>{l()}</p>}</Show>
      </header>
      <Show when={endsAt()}>{(t) => <p class="clock num">Ends at {t()}</p>}</Show>

      <Show when={status()}>
        {(s) => (
          <section class="pstate" role="status">
            <p class={`st ${s().tone}`}>
              <Show when={s().busy} fallback={<CircleAlert aria-hidden />}>
                <LoaderCircle aria-hidden class="spin" />
              </Show>
              {s().text}
            </p>
            <Show when={snap()?.attempts.length}>
              <ul class="attempts">
                <For each={snap()?.attempts}>
                  {(a) => (
                    <li>
                      <b>{a.via}</b>
                      <span>{a.error}</span>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
            <Show when={!s().busy}>
              <div class="acts">
                <A class="btn primary" href={versionsPath(props.kind, props.id, props.videoId)} data-f="player:versions" data-autofocus>
                  <Layers aria-hidden />
                  Choose another version
                </A>
                <A class="btn" href="/addons" data-f="player:addons">
                  Addons
                </A>
              </div>
            </Show>
          </section>
        )}
      </Show>

      <Show when={subError()}>
        <p class="toast bad" role="status">
          <CircleAlert aria-hidden />
          {subError()}
        </p>
      </Show>

      <Show when={url()}>
        <section class="osd" aria-label="Playback controls">
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
                if (video && duration()) video.currentTime = ((e.clientX - r.left) / r.width) * duration();
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
              </button>
              <button class="round main" data-f="player:toggle" data-autofocus aria-label={paused() ? "Play" : "Pause"} onClick={toggle}>
                <Show when={paused()} fallback={<Pause aria-hidden class="fill" />}>
                  <Play aria-hidden class="fill" />
                </Show>
              </button>
              <button class="round" data-f="player:fwd10" aria-label="Forward 10 seconds" onClick={() => seek(10)}>
                <RotateCw aria-hidden />
              </button>
            </div>
            <div class="pacts">
              <button class="pa" data-f="player:subs" onClick={() => setPanel(panel() ? null : "subs")} aria-expanded={panel() === "subs"}>
                <Captions aria-hidden />
                <b>Subtitles</b>
                <small>{track() ? languageName(track()?.lang ?? "") : snap()?.subtitlesPending ? "Searching" : `Off, ${snap()?.subtitles.length ?? 0} found`}</small>
              </button>
              <A class="pa" href={versionsPath(props.kind, props.id, props.videoId)} data-f="player:versionsact">
                <Layers aria-hidden />
                <b>Versions</b>
                <small>{playing()?.quality ?? ""}</small>
              </A>
              <Show when={ctx.ref}>
                {(r) => (
                  <A class="pa" href={titlePath(props.kind, props.id)} data-f="player:episodes">
                    <ListVideo aria-hidden />
                    <b>Episodes</b>
                    <small>{seasonName(r().season)}</small>
                  </A>
                )}
              </Show>
              <button class="pa" data-f="player:speed" onClick={cycleSpeed}>
                <Gauge aria-hidden />
                <b>Speed</b>
                <small class="num">{rate()}x</small>
              </button>
            </div>
          </div>
        </section>
      </Show>

      <Show when={panel() === "subs"}>
        <aside class="panel" aria-label="Subtitles" data-fgroup="subs">
          <h2>Subtitles</h2>
          <div class="panel-list" data-vscroll>
            <button class="opt" classList={{ on: !track() }} data-f="sub:off" data-autofocus onClick={() => pickSubtitle(null)}>
              Off
            </button>
            <For each={snap()?.subtitles ?? []}>
              {(t, i) => (
                <button class="opt" data-f={`sub:${i()}`} onClick={() => pickSubtitle(t)}>
                  <b>{languageName(t.lang)}</b>
                  <span>
                    {[t.label, t.source, t.forced ? "Forced" : null, t.hearingImpaired ? "For the hard of hearing" : null].filter(Boolean).join(", ")}
                  </span>
                </button>
              )}
            </For>
            <Show when={snap()?.subtitlesPending}>
              <p class="opt-note">Still asking subtitle addons</p>
            </Show>
          </div>
        </aside>
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
