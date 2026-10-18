/** The player's one seam to whatever decodes the video: the `<video>` element
 * in the browser (and Playwright), libmpv under a transparent page in the
 * desktop shells (plans/m3-desktop.md section 5). The player screen talks to
 * an Engine only; everything engine-specific stays behind it. */

import type { Accessor, JSX } from "solid-js";
import type { PlayerFailure } from "../lib/failover";

/** An audio or subtitle track the engine itself knows, as mpv lists them
 * (`track-list`). The `<video>` engine knows none: Chromium lists no audio
 * tracks, and its subtitles are the core's WebVTT list, attached with
 * `setSubtitle`. The player shows these in its Audio and Subtitles panels. */
export interface Track {
  id: string;
  kind: "audio" | "sub";
  /** ISO 639 code when the file names one. */
  lang: string | null;
  /** "English 5.1", "Commentary": what the file calls it. */
  label: string;
  selected: boolean;
}

/** What the engine reports back; the player decides what each means. */
export interface EngineEvents {
  /** Playback started or resumed. */
  onPlay(): void;
  onPause(): void;
  /** The end of the file. */
  onEnded(): void;
  /** The source cannot play here: the player fails over. */
  onError(failure: PlayerFailure): void;
  /** The file's length is known (after any resume seek); may repeat. */
  onLoaded(): void;
  /** A subtitle file did not load. */
  onSubtitleError(lang: string): void;
}

export interface Engine {
  /** "mpv" draws under a transparent page; "video" is an element in it. */
  kind: "video" | "mpv";
  /** What the player renders where the picture goes. */
  view: JSX.Element;
  /** The playhead for display, updated a few times a second. */
  time: Accessor<number>;
  /** The playhead right now (not reactive), for progress reports and
   * relative seeks right after another seek. */
  position(): number;
  duration: Accessor<number>;
  paused: Accessor<boolean>;
  buffered: Accessor<[number, number][]>;
  /** The first frame is on screen. */
  started: Accessor<boolean>;
  rate: Accessor<number>;
  tracks: Accessor<Track[]>;
  /** A new source (first start or failover), from `startSec`, then plays. */
  load(url: string, startSec: number): void;
  play(): void;
  pause(): void;
  toggle(): void;
  /** Absolute position in seconds. */
  seek(sec: number): void;
  setRate(rate: number): void;
  setVolume(percent: number): void;
  /** One of the core's WebVTT tracks, or none. */
  setSubtitle(sub: { url: string; lang: string } | null): void;
  /** One of the engine's own tracks (`tracks`). */
  selectTrack(kind: Track["kind"], id: string): void;
  /** Leaving the player: stop and let go of the source. */
  stop(): void;
}
