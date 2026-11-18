/** HLS where the browser has none of its own (Firefox, Chrome before 142):
 * hls.js over Media Source Extensions, loaded only when such a source plays.
 * Safari and Chrome 142+ play HLS themselves and keep doing so, as does LG
 * webOS, whose build leaves hls.js out. */

import type { PlaybackSnapshot } from "../core/generated/PlaybackSnapshot";

const HLS_TYPE = "application/vnd.apple.mpegurl";

/** The browser plays HLS in a plain `<video>`. */
export const nativeHls = () => document.createElement("video").canPlayType(HLS_TYPE) !== "";

/** Media Source Extensions, which hls.js plays through. */
export const mediaSource = () => typeof MediaSource !== "undefined" || "ManagedMediaSource" in globalThis;

/** Whether the source the core has ready is an HLS lease (a manifest it serves as `m3u8`). While none is ready (the
 * start, a failover, Try again) the answer stays `was`, the last source's, so what hangs on it never shows and hides
 * again; undefined before the first. */
export const hlsLease = (s: Pick<PlaybackSnapshot, "phase" | "manifest" | "container"> | null, was?: boolean) => (s?.phase === "ready" ? s.manifest && s.container === "m3u8" : was);

/** An HLS playlist: the core names every one `.m3u8`. */
export const isHls = (url: string) => /\.m3u8$/i.test(new URL(url, location.href).pathname);

/** An HLS playlist this browser plays only through hls.js. */
export const needsHlsJs = (url: string) => isHls(url) && !nativeHls() && mediaSource();
