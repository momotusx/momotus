/** The one YouTube player the app runs (plans/appletv-polish.md 4.2, decisions 6 and 15; plans/trailer-inline.md):
 * YouTube's own embedded player on its privacy-enhanced host, muted and read back before it plays, taking no input,
 * one at a time app-wide. The hero and a title's header play it muted under their art; a title's Trailer plays it
 * full screen (`full`) under our own controls, with its own sound. */
import { batch, createSignal } from "solid-js";
import { walkNow, youtubeId } from "../lib/trailers";

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
  pauseVideo(): void;
  seekTo(seconds: number, allowSeekAhead: boolean): void;
  getPlayerState(): number;
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

/** Where a full-screen trailer is, read every 250 ms: seconds in, its length (0 until known), and its state. */
export interface TrailerTime {
  t: number;
  d: number;
  state: "playing" | "paused" | "buffering";
}

/** What a full-screen trailer's controls drive. */
export interface TrailerControl {
  stop: () => void;
  toggle: () => void;
  /** To `s` seconds; `done` false while a drag still moves (YouTube fetches nothing new until it is let go). */
  seek: (s: number, done?: boolean) => void;
  sound: (on: boolean) => void;
}

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

/** Plays `id` muted in `host` (an empty box the layout keeps clear), capped at `capS` seconds, 0 for its own length
 * (it still ends a second before, so YouTube's end screen never shows). `onPlaying` fires once YouTube says it plays;
 * `onTime` gives the place in the cap; `onEnd` says why it stopped. Starting another stops this one first.
 * `full`: the full-screen trailer. It reports `onState`, never touches the session's sound or the header's switch,
 * and unmutes once it plays when `sound` says so. */
export function playTrailer(opts: {
  host: HTMLElement;
  id: string;
  capS: number;
  onPlaying: () => void;
  onTime: (fraction: number) => void;
  onEnd: (why: TrailerEnd) => void;
  full?: { sound: () => boolean; onState: (s: TrailerTime) => void };
}): TrailerControl {
  current?.stop("stopped");
  const id = youtubeId(opts.id);
  const box = document.createElement("div");
  opts.host.replaceChildren(box);
  const timers: ReturnType<typeof setTimeout>[] = [];
  let poll: ReturnType<typeof setInterval> | undefined;
  let ended = false;
  let playingNow = false;
  const full = opts.full;
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
      // Only its own frame goes: a player started meanwhile in the same host stays.
      const gone = () => {
        try {
          player?.destroy();
        } catch {
          // Already gone with its iframe.
        }
        box.remove();
      };
      // The art fades back in over 600 ms above a player that still plays, never over black.
      if (why === "cap" || why === "ended") setTimeout(gone, 650);
      else gone();
      // One batch: the end hears it while the sound switch is still drawn, so a focus on it can move to Trailer.
      batch(() => {
        if (current === self) {
          current = null;
          setPlaying(false);
        }
        opts.onEnd(why);
      });
    },
  };
  current = self;
  const control: TrailerControl = {
    stop: () => self.stop("stopped"),
    toggle: () => {
      const p = self.player;
      if (!p) return;
      if (p.getPlayerState() === 1) p.pauseVideo();
      else p.playVideo();
    },
    seek: (s, done = true) => self.player?.seekTo(Math.max(0, s), done),
    sound: (on) => {
      const p = self.player;
      if (!p || walkNow()) return;
      if (on) {
        p.unMute();
        p.setVolume(100);
      } else {
        p.mute();
        p.setVolume(0);
      }
    },
  };
  if (!id) {
    queueMicrotask(() => self.stop("error"));
    return control;
  }
  const cap = opts.capS;
  // Full screen: nothing playing 15 s after the press, the script's load included, gives up (YouTube unreachable). A
  // preview keeps its art meanwhile and times out from its play (below).
  if (full) timers.push(setTimeout(() => !playingNow && self.stop("timeout"), 15_000));
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
                if (!full && !muted()) setMuted(false);
                p.playVideo();
                if (!full) timers.push(setTimeout(() => self.player && !playingNow && self.stop("timeout"), 6000));
              } else if (performance.now() < until) timers.push(setTimeout(check, 25));
              else self.stop("error");
            };
            check();
          },
          onStateChange: (e: { data: number }) => {
            if (ended) return;
            if (e.data === YT.PlayerState.PLAYING && !playingNow) {
              playingNow = true;
              if (full) {
                if (full.sound()) control.sound(true);
              } else setPlaying(true);
              opts.onPlaying();
              poll = setInterval(() => {
                const p = self.player;
                if (!p) return;
                const length = cap || p.getDuration();
                const t = p.getCurrentTime();
                opts.onTime(length > 0 ? Math.min(1, t / length) : 0);
                const state = p.getPlayerState();
                full?.onState({ t, d: length, state: state === YT.PlayerState.PLAYING ? "playing" : state === 2 ? "paused" : "buffering" });
                if (Math.round(t * 4) % 4 === 0) captionsOff();
                // Back to the art (or out of full screen) 1 s before the end, so YouTube's end screen never shows.
                if (length > 0 && t >= length - 1) self.stop(cap ? "cap" : "ended");
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
  return control;
}
