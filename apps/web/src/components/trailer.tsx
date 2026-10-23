/** A title's trailer over the page: the YouTube privacy-enhanced player,
 * with focus kept on our own buttons so a remote's Back always reaches us.
 * Back or Escape closes it; Pause on the bar, a remote's play/pause key, K
 * and Space pause and play it through YouTube's player API. */

import Pause from "lucide-solid/icons/pause";
import Play from "lucide-solid/icons/play";
import X from "lucide-solid/icons/x";
import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import type { Trailer } from "../core/generated/Trailer";
import { pushBack } from "../focus/back";
import { focusEl } from "../focus/dom";

const YOUTUBE = "https://www.youtube-nocookie.com";
const PLAY_KEYS = new Set(["MediaPlayPause", "MediaPlay", "MediaPause", "k", "K", " "]);

export function TrailerOverlay(props: { trailers: Trailer[]; onClose: () => void }) {
  const [index, setIndex] = createSignal(0);
  const current = () => props.trailers[index()] ?? props.trailers[0];
  // It starts playing (autoplay); each press asks the player for the other state.
  const [playing, setPlaying] = createSignal(true);
  let frame: HTMLIFrameElement | undefined;
  const toggle = () => {
    const next = !playing();
    frame?.contentWindow?.postMessage(JSON.stringify({ event: "command", func: next ? "playVideo" : "pauseVideo", args: [] }), YOUTUBE);
    setPlaying(next);
  };
  onMount(() => {
    // Back closes it through the one Back stack, so a dialog opened over it closes first.
    const removeBack = pushBack(() => props.onClose());
    const onKey = (e: KeyboardEvent) => {
      // Space on one of the bar's buttons stays that button's press.
      if (PLAY_KEYS.has(e.key) && !(e.key === " " && (e.target as Element | null)?.closest?.("button"))) {
        e.preventDefault();
        e.stopImmediatePropagation();
        toggle();
      }
    };
    window.addEventListener("keydown", onKey, { capture: true });
    queueMicrotask(() => {
      const close = document.querySelector<HTMLElement>('[data-f="trailer:close"]');
      if (close) focusEl(close);
    });
    onCleanup(() => {
      removeBack();
      window.removeEventListener("keydown", onKey, { capture: true });
    });
  });
  return (
    <div class="trailer" role="dialog" aria-modal="true" aria-label={`Trailer: ${current()?.title ?? ""}`}>
      <div class="trailer-frame">
        <Show when={current()} keyed>
          {(t) => (
            <iframe
              ref={(el) => {
                frame = el;
                setPlaying(true);
              }}
              src={`${YOUTUBE}/embed/${encodeURIComponent(t.youtubeId)}?autoplay=1&rel=0&playsinline=1&enablejsapi=1&origin=${encodeURIComponent(location.origin)}`}
              title={`Trailer: ${t.title}`}
              allow="autoplay; encrypted-media; picture-in-picture; fullscreen"
              referrerpolicy="strict-origin-when-cross-origin"
              tabindex="-1"
            />
          )}
        </Show>
      </div>
      <div class="trailer-bar" data-fgroup="trailer">
        <p>
          <span class="kicker">Trailer</span>
          {current()?.title}
        </p>
        <Show when={props.trailers.length > 1}>
          <div class="trailer-picks" role="radiogroup" aria-label="Trailers">
            <For each={props.trailers}>
              {(_, i) => (
                <button class="chip" classList={{ on: i() === index() }} role="radio" aria-checked={i() === index()} data-f={`trailer:${i()}`} onClick={() => setIndex(i())}>
                  {i() === 0 ? "Main trailer" : `Trailer ${i() + 1}`}
                </button>
              )}
            </For>
          </div>
        </Show>
        <button class="btn" data-f="trailer:toggle" onClick={toggle}>
          <Show when={playing()} fallback={<Play aria-hidden class="fill" />}>
            <Pause aria-hidden class="fill" />
          </Show>
          {playing() ? "Pause" : "Play"}
        </button>
        <button class="btn" data-f="trailer:close" onClick={props.onClose}>
          <X aria-hidden />
          Close
        </button>
      </div>
    </div>
  );
}
