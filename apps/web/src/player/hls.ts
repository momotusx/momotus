/** HLS where the browser has none of its own (Firefox, Chrome before 142):
 * hls.js over Media Source Extensions, loaded only when such a source plays.
 * Safari and Chrome 142+ play HLS themselves and keep doing so. */

const HLS_TYPE = "application/vnd.apple.mpegurl";

/** The browser plays HLS in a plain `<video>`. */
export const nativeHls = () => document.createElement("video").canPlayType(HLS_TYPE) !== "";

/** Media Source Extensions, which hls.js plays through. */
export const mediaSource = () => typeof MediaSource !== "undefined" || "ManagedMediaSource" in globalThis;

/** An HLS playlist (the core names every one `.m3u8`) this browser plays only through hls.js. */
export const needsHlsJs = (url: string) => /\.m3u8$/i.test(new URL(url, location.href).pathname) && !nativeHls() && mediaSource();
