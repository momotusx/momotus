/** The browser's engine: one `<video>` element. The core serves every source
 * and subtitle on loopback URLs any origin may read, and the element is
 * crossorigin, so both load directly. */

import { createSignal, Show } from "solid-js";
import { earlyFailure, mediaFailure, pendingStart } from "../lib/failover";
import { startAudio } from "../lib/words";
import type { Engine, EngineEvents, Track } from "./engine";

export function createVideoEngine(events: EngineEvents): Engine {
  let video!: HTMLVideoElement;
  const [time, setTime] = createSignal(0);
  const [duration, setDuration] = createSignal(0);
  const [paused, setPaused] = createSignal(true);
  const [buffered, setBuffered] = createSignal<[number, number][]>([]);
  const [started, setStarted] = createSignal(false);
  const [waiting, setWaiting] = createSignal(false);
  const [rate, setRate] = createSignal(1);
  // The two subtitle lines' files (plans/dual-subtitles.md), each its own <track>, read by its element.
  const [subtitle, setSubtitle] = createSignal<{ url: string; lang: string } | null>(null);
  const [second, setSecond] = createSignal<{ url: string; lang: string } | null>(null);
  const lines: Record<1 | 2, HTMLTrackElement | undefined> = { 1: undefined, 2: undefined };
  // Audio tracks where the browser lists them (Safari; Chromium only behind a
  // flag); subtitles are the core's WebVTT list, kept by the player.
  const [tracks, setTracks] = createSignal<Track[]>([]);
  type AudioTrackLike = { id: string; language: string; label: string; enabled: boolean };
  const audioList = () => (video as unknown as { audioTracks?: ArrayLike<AudioTrackLike> }).audioTracks;
  const readTracks = () => {
    const list = audioList();
    setTracks(
      list
        ? Array.from(list, (t) => ({ id: t.id, kind: "audio" as const, lang: t.language || null, label: t.label, selected: t.enabled }))
        : [],
    );
  };

  // The core's start audio (`audioLanguages`): the first of them the file lists turns on as it opens.
  let wantedAudio: string[] = [];
  // The viewer picked a track in this file: a later start audio never goes over it.
  let picked = false;
  const applyStartAudio = () => {
    if (picked) return;
    const list = audioList();
    const all = list ? Array.from(list) : [];
    const id = startAudio(all, wantedAudio);
    if (id) for (const t of all) t.enabled = t.id === id;
  };

  // Each line's delay moves its cues from the time their file gave them.
  const delays: Record<1 | 2, number> = { 1: 0, 2: 0 };
  const given = new WeakMap<TextTrackCue, [number, number]>();
  const shiftCues = (line: 1 | 2) => {
    const cues = lines[line]?.track.cues;
    if (!cues) return;
    for (const cue of Array.from(cues)) {
      let at = given.get(cue);
      if (!at) {
        at = [cue.startTime, cue.endTime];
        given.set(cue, at);
      }
      cue.startTime = Math.max(0, at[0] + delays[line]);
      cue.endTime = Math.max(0, at[1] + delays[line]);
    }
  };
  // The page draws the lines itself (player.tsx): each track runs hidden (a disabled one loads no cues).
  const hide = (line: 1 | 2) => queueMicrotask(() => lines[line] && (lines[line].track.mode = "hidden"));
  const trackEl = (line: 1 | 2, t: { url: string; lang: string }) => (
    <track
      ref={(el) => (lines[line] = el)}
      kind="subtitles"
      data-line={line}
      src={t.url}
      srclang={t.lang}
      onLoad={() => shiftCues(line)}
      onError={() => events.onSubtitleError(t.lang, line)}
    />
  );

  const start = pendingStart();
  // The listener that applies the loading source's start; a source that
  // fails before its metadata must not leave it to seek the next one.
  let onMetadata: (() => void) | null = null;

  const known = () => Number.isFinite(video?.duration) && video.duration > 0;
  const seek = (sec: number) => {
    if (video && !start.retarget(sec)) video.currentTime = Math.max(0, sec);
  };
  // Not paused from the press on (the play event comes a task later), and
  // paused again when the browser refuses (it sends no event then).
  const play = () => {
    if (!video) return;
    setPaused(false);
    void video.play().catch(() => setPaused(video.paused));
  };

  const view = (
    <video
      ref={video}
      class="video"
      classList={{ on: started() }}
      playsinline
      preload="auto"
      crossorigin="anonymous"
      onPlay={() => {
        setPaused(false);
        events.onPlay();
      }}
      onPlaying={() => {
        setStarted(true);
        setWaiting(false);
      }}
      onWaiting={() => setWaiting(true)}
      onEmptied={() => setWaiting(false)}
      onPause={() => {
        setPaused(true);
        events.onPause();
      }}
      onTimeUpdate={() => setTime(video.currentTime)}
      onDurationChange={() => {
        setDuration(known() ? video.duration : 0);
        if (known()) events.onLoaded();
      }}
      onProgress={() => {
        const out: [number, number][] = [];
        for (let i = 0; i < video.buffered.length; i++) out.push([video.buffered.start(i), video.buffered.end(i)]);
        setBuffered(out);
      }}
      onEnded={() => events.onEnded()}
      onError={() => {
        const error = video.error;
        // Before the metadata, the element cannot tell a failed fetch from a file it cannot read.
        if (video.readyState >= HTMLMediaElement.HAVE_METADATA || !video.currentSrc) events.onError(mediaFailure(error));
        else void earlyFailure(video.currentSrc, error).then(events.onError);
      }}
    >
      <Show when={subtitle()} keyed>
        {(t) => trackEl(1, t)}
      </Show>
      <Show when={second()} keyed>
        {(t) => trackEl(2, t)}
      </Show>
    </video>
  ) as HTMLVideoElement;

  return {
    kind: "video",
    view,
    time,
    position: () => start.position(video?.currentTime ?? 0),
    duration,
    paused,
    buffered,
    started,
    waiting,
    rate,
    tracks,
    load(url, startSec, hold = false) {
      if (onMetadata) video.removeEventListener("loadedmetadata", onMetadata);
      picked = false;
      start.set(startSec);
      video.src = url;
      const applyStart = () => {
        onMetadata = null;
        const at = start.take();
        if (at != null && at > 0) video.currentTime = at;
        applyStartAudio();
        readTracks();
        events.onLoaded();
      };
      onMetadata = applyStart;
      video.addEventListener("loadedmetadata", applyStart, { once: true });
      if (hold) setPaused(true);
      else play();
    },
    play,
    pause: () => video?.pause(),
    toggle() {
      if (!video) return;
      if (video.paused) play();
      else video.pause();
    },
    seek,
    setRate(r) {
      video.playbackRate = r;
      setRate(r);
    },
    setVolume(percent) {
      video.volume = Math.min(1, Math.max(0, percent / 100));
    },
    setSubtitle(sub, line = 1) {
      if (line === 1) setSubtitle(sub);
      else setSecond(sub);
      if (!sub) lines[line] = undefined;
      else hide(line);
    },
    setSubtitleDelay(sec, line = 1) {
      delays[line] = sec;
      shiftCues(line);
    },
    cueTrack: (line) => lines[line]?.track ?? null,
    setAudioLanguages(languages) {
      wantedAudio = languages;
      applyStartAudio();
      readTracks();
    },
    selectTrack(kind, id) {
      const list = audioList();
      if (kind !== "audio" || !list) return;
      picked = true;
      for (const t of Array.from(list)) t.enabled = t.id === id;
      readTracks();
    },
    stop() {
      // The element leaves with the page and takes its source along; pausing
      // here would report to a view that is already closing.
    },
  };
}
