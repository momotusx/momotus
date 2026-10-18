/** The browser's engine: one `<video>` element. The core serves every source
 * and subtitle on loopback URLs any origin may read, and the element is
 * crossorigin, so both load directly. */

import { createSignal, Show } from "solid-js";
import { mediaFailure, pendingStart } from "../lib/failover";
import type { Engine, EngineEvents, Track } from "./engine";

export function createVideoEngine(events: EngineEvents): Engine {
  let video!: HTMLVideoElement;
  const [time, setTime] = createSignal(0);
  const [duration, setDuration] = createSignal(0);
  const [paused, setPaused] = createSignal(true);
  const [buffered, setBuffered] = createSignal<[number, number][]>([]);
  const [started, setStarted] = createSignal(false);
  const [rate, setRate] = createSignal(1);
  const [subtitle, setSubtitle] = createSignal<{ url: string; lang: string } | null>(null);
  // Audio tracks where the browser lists them (Safari; Chromium only behind a
  // flag); subtitles are the core's WebVTT list, kept by the player.
  const [tracks, setTracks] = createSignal<Track[]>([]);
  type AudioTrackLike = { id: string; language: string; label: string; enabled: boolean };
  const audioList = () => (video as unknown as { audioTracks?: ArrayLike<AudioTrackLike> }).audioTracks;
  const readTracks = () => {
    const list = audioList();
    setTracks(
      list
        ? Array.from(list, (t) => ({ id: t.id, kind: "audio" as const, lang: t.language || null, label: t.label || t.language || "Track", selected: t.enabled }))
        : [],
    );
  };

  const start = pendingStart();
  // The listener that applies the loading source's start; a source that
  // fails before its metadata must not leave it to seek the next one.
  let onMetadata: (() => void) | null = null;

  const known = () => Number.isFinite(video?.duration) && video.duration > 0;
  const seek = (sec: number) => {
    if (video && !start.retarget(sec)) video.currentTime = Math.max(0, sec);
  };
  const play = () => void video?.play().catch(() => setPaused(true));

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
      onPlaying={() => setStarted(true)}
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
      onError={() => events.onError(mediaFailure(video.error))}
    >
      <Show when={subtitle()} keyed>
        {(t) => <track kind="subtitles" src={t.url} srclang={t.lang} default onError={() => events.onSubtitleError(t.lang)} />}
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
    rate,
    tracks,
    load(url, startSec) {
      if (onMetadata) video.removeEventListener("loadedmetadata", onMetadata);
      start.set(startSec);
      video.src = url;
      const applyStart = () => {
        onMetadata = null;
        const at = start.take();
        if (at != null && at > 0) video.currentTime = at;
        readTracks();
        events.onLoaded();
      };
      onMetadata = applyStart;
      video.addEventListener("loadedmetadata", applyStart, { once: true });
      play();
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
    setSubtitle(sub) {
      setSubtitle(sub);
      if (sub)
        queueMicrotask(() => {
          const tt = video.textTracks[0];
          if (tt) tt.mode = "showing";
        });
    },
    selectTrack(kind, id) {
      const list = audioList();
      if (kind !== "audio" || !list) return;
      for (const t of Array.from(list)) t.enabled = t.id === id;
      readTracks();
    },
    stop() {
      // The element leaves with the page and takes its source along; pausing
      // here would report to a view that is already closing.
    },
  };
}
