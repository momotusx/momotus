/** The one muted YouTube player the hero may run (plans/appletv-polish.md 4.2, decisions 6 and 15):
 * YouTube's own embedded player on its privacy-enhanced host, loaded paused under the hero's art, muted
 * and read back before it plays, taking no input, one at a time app-wide. The hero decides when the art
 * above it fades. */
import { createSignal } from "solid-js";
import { youtubeId } from "../lib/trailers";

type YtPlayer = {
  unloadModule(name: string): void;
  getOption(module: string, option: string): unknown;
  setOption(module: string, option: string, value: unknown): void;
  mute(): void;
  unMute(): void;
  isMuted(): boolean;
  setVolume(v: number): void;
  getVolume(): number;
  playVideo(): void;
  getCurrentTime(): number;
  getDuration(): number;
  getIframe(): HTMLIFrameElement;
  destroy(): void;
};
type YtNamespace = {
  Player: new (el: HTMLElement, opts: Record<string, unknown>) => YtPlayer;
  PlayerState: { PLAYING: number; ENDED: number };
};
declare global {
  interface Window {
    YT?: YtNamespace;
    onYouTubeIframeAPIReady?: () => void;
  }
}

export type TrailerEnd = "ended" | "cap" | "error" | "timeout" | "stopped";

/** A trailer is on screen: the mute control shows (phone top row, desktop header). */
const [playing, setPlaying] = createSignal(false);
/** Sound stays off unless the viewer turns it on, for this page's life only. */
const [muted, setMutedSignal] = createSignal(true);
export { muted, playing as trailerPlaying };

let api: Promise<YtNamespace> | null = null;
function loadApi(): Promise<YtNamespace> {
  api ??= new Promise((resolve, reject) => {
    if (window.YT?.Player) return resolve(window.YT);
    const previous = window.onYouTubeIframeAPIReady;
    window.onYouTubeIframeAPIReady = () => {
      previous?.();
      if (window.YT) resolve(window.YT);
    };
    const s = document.createElement("script");
    s.src = "https://www.youtube.com/iframe_api";
    s.onerror = () => {
      api = null;
      reject(new Error("YouTube's player did not load"));
    };
    document.head.append(s);
  });
  return api;
}

let current: { stop: (why: TrailerEnd) => void; player: YtPlayer | null } | null = null;

/** Stops whatever plays: a page change, a page left, Home covered. */
export function stopTrailer() {
  current?.stop("stopped");
}

/** The viewer's sound switch; never reachable in a walk (the control is not drawn there). */
export function setMuted(on: boolean) {
  setMutedSignal(on);
  const p = current?.player;
  if (!p) return;
  if (on) {
    p.mute();
    p.setVolume(0);
  } else {
    p.unMute();
    p.setVolume(100);
  }
}

/** Plays `id` muted in `host` (an empty box the layout keeps clear), capped at `capS` seconds.
 * `onPlaying` fires once YouTube says it plays; `onTime` gives the place in the cap; `onEnd` says why
 * it stopped. Starting another stops this one first. */
export function playTrailer(opts: {
  host: HTMLElement;
  id: string;
  capS: number;
  onPlaying: () => void;
  onTime: (fraction: number) => void;
  onEnd: (why: TrailerEnd) => void;
}): () => void {
  current?.stop("stopped");
  const id = youtubeId(opts.id);
  const box = document.createElement("div");
  opts.host.replaceChildren(box);
  const timers: ReturnType<typeof setTimeout>[] = [];
  let poll: ReturnType<typeof setInterval> | undefined;
  let ended = false;
  let playingNow = false;
  /** YouTube turns a caption track on for muted playback now and then: it goes again (Phase 0: 1 of 24). */
  const captionsOff = () => {
    const p = self.player;
    if (!p) return;
    try {
      const track = p.getOption("captions", "track");
      if (track && typeof track === "object" && Object.keys(track).length) p.setOption("captions", "track", {});
      p.unloadModule("captions");
    } catch {
      // The module is not there yet.
    }
  };
  const self = {
    player: null as YtPlayer | null,
    stop: (why: TrailerEnd) => {
      if (ended) return;
      ended = true;
      timers.forEach(clearTimeout);
      clearInterval(poll);
      const player = self.player;
      self.player = null;
      const gone = () => {
        try {
          player?.destroy();
        } catch {
          // Already gone with its iframe.
        }
        opts.host.replaceChildren();
      };
      // The art fades back in over 600 ms above a player that still plays, never over black.
      if (why === "cap" || why === "ended") setTimeout(gone, 650);
      else gone();
      if (current === self) {
        current = null;
        setPlaying(false);
      }
      opts.onEnd(why);
    },
  };
  current = self;
  if (!id) {
    queueMicrotask(() => self.stop("error"));
    return () => self.stop("stopped");
  }
  const cap = opts.capS;
  loadApi().then(
    (YT) => {
      if (ended) return;
      self.player = new YT.Player(box, {
        host: "https://www.youtube-nocookie.com",
        videoId: id,
        width: "100%",
        height: "100%",
        playerVars: {
          controls: 0,
          playsinline: 1,
          rel: 0,
          enablejsapi: 1,
          iv_load_policy: 3,
          cc_load_policy: 0,
          disablekb: 1,
          fs: 0,
          origin: location.origin,
          ...(cap ? { end: cap } : {}),
        },
        events: {
          onReady: () => {
            const p = self.player;
            if (!p || ended) return;
            // It takes no input: keys, swipes and the remote stay ours (decision 15).
            const frame = p.getIframe();
            frame.setAttribute("tabindex", "-1");
            frame.inert = true;
            frame.setAttribute("allow", "autoplay; encrypted-media");
            frame.setAttribute("aria-hidden", "true");
            p.mute();
            p.setVolume(0);
            // isMuted answers from the player's last message, so the read-back waits for it: never
            // play before it says muted with no volume (decision 6; 26 to 55 ms in Phase 0).
            const until = performance.now() + 1000;
            const check = () => {
              if (ended || !self.player) return;
              if (p.isMuted() && p.getVolume() === 0) {
                if (!muted()) setMuted(false);
                p.playVideo();
                timers.push(setTimeout(() => self.player && !playingNow && self.stop("timeout"), 6000));
              } else if (performance.now() < until) timers.push(setTimeout(check, 25));
              else self.stop("error");
            };
            check();
          },
          onStateChange: (e: { data: number }) => {
            if (ended) return;
            if (e.data === YT.PlayerState.PLAYING && !playingNow) {
              playingNow = true;
              setPlaying(true);
              opts.onPlaying();
              poll = setInterval(() => {
                const p = self.player;
                if (!p) return;
                const length = cap || p.getDuration() || 1;
                const t = p.getCurrentTime();
                opts.onTime(Math.min(1, t / length));
                if (Math.round(t * 4) % 4 === 0) captionsOff();
                // Back to the art 1 s before the cap, so YouTube's end screen never shows.
                if (cap && t >= cap - 1) self.stop("cap");
              }, 250);
            }
            if (e.data === YT.PlayerState.ENDED) self.stop(cap ? "cap" : "ended");
          },
          onApiChange: () => captionsOff(),
          onError: () => self.stop("error"),
        },
      });
    },
    () => self.stop("error"),
  );
  return () => self.stop("stopped");
}
