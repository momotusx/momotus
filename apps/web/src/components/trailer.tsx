/** A title's trailer over the page (plans/trailer-inline.md 7): YouTube's own player on its privacy host, taking no
 * input, under our own controls, so a remote's Back, the focus and screen readers stay with us and nothing ever
 * leaves for YouTube. Back or Escape closes it; Play/Pause, the remote's play key, K and Space pause and play it;
 * the position bar seeks (Left and Right 10 s, a press or a drag), as do the remote's rewind and fast forward. A
 * trailer YouTube will not play here gives way to the next one, then to a quiet line. */

import LoaderCircle from "lucide-solid/icons/loader-circle";
import Pause from "lucide-solid/icons/pause";
import Play from "lucide-solid/icons/play";
import Volume2 from "lucide-solid/icons/volume-2";
import VolumeX from "lucide-solid/icons/volume-x";
import X from "lucide-solid/icons/x";
import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import type { Trailer } from "../core/generated/Trailer";
import { pushBack } from "../focus/back";
import { focusEl } from "../focus/dom";
import { form } from "../form";
import { clock, pct } from "../lib/format";
import { seekTarget, walkNow } from "../lib/trailers";
import { playTrailer, type TrailerControl, type TrailerTime } from "./yt-player";

const PLAY_KEYS = new Set(["MediaPlayPause", "MediaPlay", "MediaPause", "k", "K", " "]);
const SEEK_S = 10;

export function TrailerOverlay(props: { trailers: Trailer[]; onClose: () => void }) {
  const [index, setIndex] = createSignal(0);
  const current = () => props.trailers[index()] ?? props.trailers[0];
  const [time, setTime] = createSignal<TrailerTime>({ t: 0, d: 0, state: "buffering" });
  // Where a drag holds the bar until it is let go.
  const [held, setHeld] = createSignal<number | null>(null);
  const [failed, setFailed] = createSignal(false);
  // Sound on: the viewer asked for this trailer. Its own, never the session's (the hero and headers stay muted).
  const [sound, setSound] = createSignal(!walkNow());
  let host!: HTMLDivElement;
  let control: TrailerControl | null = null;
  let run = 0;

  const start = (i: number) => {
    const my = ++run;
    control?.stop();
    setIndex(i);
    setFailed(false);
    setTime({ t: 0, d: 0, state: "buffering" });
    const t = props.trailers[i];
    if (!t) return;
    control = playTrailer({
      host,
      id: t.youtubeId,
      capS: 0,
      onPlaying: () => undefined,
      onTime: () => undefined,
      full: { sound, onState: (s) => my === run && setTime(s) },
      onEnd: (why) => {
        if (my !== run) return;
        control = null;
        // Its end, or stopped from outside (the tab hidden for good, a film starting): the overlay goes with it.
        if (why !== "error" && why !== "timeout") return props.onClose();
        // Refused here (embedding off, gone) or never ready: the next trailer, then the quiet line.
        if (i + 1 < props.trailers.length) return start(i + 1);
        setFailed(true);
        queueMicrotask(() => {
          const close = document.querySelector<HTMLElement>('[data-f="trailer:close"]');
          if (close) focusEl(close);
        });
      },
    });
  };
  const toggle = () => control?.toggle();
  const position = () => held() ?? time().t;
  const seekBy = (by: number) => {
    const s = time();
    if (!control || s.d <= 0) return;
    const to = seekTarget(s.t, s.d, by);
    setTime({ ...s, t: to });
    control.seek(to);
  };
  const toggleSound = () => {
    const on = !sound();
    setSound(on);
    control?.sound(on);
  };
  const seekAt = (e: PointerEvent, done: boolean) => {
    const bar = e.currentTarget as HTMLElement;
    const d = time().d;
    if (d <= 0) return;
    const r = bar.getBoundingClientRect();
    const to = seekTarget(((e.clientX - r.left) / r.width) * d, d, 0);
    setHeld(done ? null : to);
    if (done) setTime({ ...time(), t: to });
    control?.seek(to, done);
  };

  onMount(() => {
    // Back closes it through the one Back stack, so a dialog opened over it closes first; the browser's Back too
    // (its own history entry).
    const removeBack = pushBack(() => props.onClose(), { entry: true });
    const onKey = (e: KeyboardEvent) => {
      // Space on one of the bar's buttons stays that button's press.
      if (PLAY_KEYS.has(e.key) && !(e.key === " " && (e.target as Element | null)?.closest?.("button"))) {
        e.preventDefault();
        e.stopImmediatePropagation();
        // Once a press: a held key never flickers it.
        if (!e.repeat) toggle();
      } else if ((e.key === "MediaRewind" || e.key === "MediaFastForward") && !e.repeat) {
        e.preventDefault();
        seekBy(e.key === "MediaRewind" ? -SEEK_S : SEEK_S);
      }
    };
    window.addEventListener("keydown", onKey, { capture: true });
    // A tab in the background pauses it.
    const hide = () => document.hidden && time().state === "playing" && toggle();
    document.addEventListener("visibilitychange", hide);
    start(0);
    // TV: the bar, where Left and Right seek; elsewhere Play/Pause.
    queueMicrotask(() => {
      const first = document.querySelector<HTMLElement>(form() === "tv" ? '[data-f="trailer:scrub"]' : '[data-f="trailer:toggle"]');
      if (first) focusEl(first);
    });
    onCleanup(() => {
      run++;
      control?.stop();
      removeBack();
      window.removeEventListener("keydown", onKey, { capture: true });
      document.removeEventListener("visibilitychange", hide);
    });
  });
  const waiting = () => !failed() && time().state === "buffering";
  return (
    <div class="trailer" role="dialog" aria-modal="true" aria-label={`Trailer: ${current()?.title ?? ""}`} data-state={failed() ? "failed" : time().state}>
      <div class="trailer-frame">
        <div class="trailer-player" aria-hidden="true">
          <div ref={host} />
        </div>
        <Show when={waiting()}>
          <LoaderCircle class="spin trailer-wait" role="img" aria-label="Loading trailer" />
        </Show>
        <Show when={failed()}>
          <p class="trailer-fail" role="alert">
            This trailer can't play here.
          </p>
        </Show>
      </div>
      <div class="trailer-tl" data-fgroup="trailer">
        <span class="num">{clock(position())}</span>
        <div
          class="scrub"
          role="slider"
          tabindex="0"
          aria-label="Position"
          aria-valuemin={0}
          aria-valuemax={Math.round(time().d)}
          aria-valuenow={Math.round(position())}
          aria-valuetext={`${clock(position())} of ${clock(time().d)}`}
          aria-disabled={failed() || time().d <= 0 || undefined}
          data-f="trailer:scrub"
          onKeyDown={(e) => {
            if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
              e.preventDefault();
              const by = e.key === "ArrowLeft" ? -SEEK_S : SEEK_S;
              // A held key moves the bar and seeks once, when it is let go: YouTube fetches one place, not thirty.
              const d = time().d;
              if (e.repeat && d > 0) setHeld(seekTarget(held() ?? time().t, d, by));
              else seekBy(by);
            } else if (e.key === "Enter") {
              e.preventDefault();
              if (!e.repeat) toggle();
            }
          }}
          // A press seeks there and a drag follows the pointer; YouTube fetches the new place once it is let go.
          onPointerDown={(e) => {
            if (e.button !== 0 || time().d <= 0) return;
            e.currentTarget.setPointerCapture(e.pointerId);
            seekAt(e, false);
          }}
          onPointerMove={(e) => e.currentTarget.hasPointerCapture(e.pointerId) && seekAt(e, false)}
          onPointerUp={(e) => held() !== null && seekAt(e, true)}
          onPointerCancel={() => setHeld(null)}
          onBlur={() => setHeld(null)}
          onKeyUp={() => {
            const to = held();
            if (to === null) return;
            setHeld(null);
            setTime({ ...time(), t: to });
            control?.seek(to);
          }}
        >
          <i class="pos" style={{ width: pct(time().d > 0 ? position() / time().d : 0) }} />
          <b class="knob" style={{ left: pct(time().d > 0 ? position() / time().d : 0) }} />
        </div>
        <span class="num">-{clock(Math.max(0, time().d - position()))}</span>
      </div>
      <div class="trailer-bar" data-fgroup="trailer">
        <p>
          <span class="kicker">Trailer</span>
          <span class="trailer-name">{current()?.title}</span>
        </p>
        <Show when={props.trailers.length > 1}>
          <div class="trailer-picks" role="radiogroup" aria-label="Trailers">
            <For each={props.trailers}>
              {(_, i) => (
                <button class="chip" classList={{ on: i() === index() }} role="radio" aria-checked={i() === index()} data-f={`trailer:${i()}`} onClick={() => start(i())}>
                  {i() === 0 ? "Main trailer" : `Trailer ${i() + 1}`}
                </button>
              )}
            </For>
          </div>
        </Show>
        <button class="btn" data-f="trailer:toggle" disabled={failed()} onClick={toggle}>
          <Show when={time().state !== "paused"} fallback={<Play aria-hidden class="fill" />}>
            <Pause aria-hidden class="fill" />
          </Show>
          {time().state === "paused" ? "Play" : "Pause"}
        </button>
        <button class="btn" data-f="trailer:sound" aria-pressed={sound()} disabled={failed()} onClick={toggleSound}>
          <Show when={sound()} fallback={<VolumeX aria-hidden />}>
            <Volume2 aria-hidden />
          </Show>
          Sound
        </button>
        <button class="btn" data-f="trailer:close" onClick={props.onClose}>
          <X aria-hidden />
          Close
        </button>
      </div>
    </div>
  );
}
