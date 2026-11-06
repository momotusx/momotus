import { expect, test } from "bun:test";
import type { PlaybackSnapshot } from "../core/generated/PlaybackSnapshot";
import { earlyFailure, hlsFailure, loadedSource, mediaFailure, pendingStart, sourceStart, tryingAgain } from "./failover";

test("a failover names the source the engine loaded, not the one the core moved to", () => {
  let snapshot = { attempt: 1 } as PlaybackSnapshot;
  const loaded = loadedSource(() => snapshot);
  loaded.load();
  snapshot = { attempt: 2 } as PlaybackSnapshot;
  expect(loaded.failOver({ kind: "decode", error: "decoder error" })).toEqual({
    type: "failOver",
    error: "decoder error",
    kind: "decode",
    attempt: 1,
  });
});

test("with no source loaded the command names none", () => {
  const loaded = loadedSource(() => null);
  const failure = { kind: "network", error: "network error" } as const;
  expect(loaded.failOver(failure)).toEqual({ type: "failOver", error: "network error", kind: "network" });
  loaded.load();
  expect(loaded.failOver(failure)).toEqual({ type: "failOver", error: "network error", kind: "network" });
});

test("an HTTP status goes with its failure", () => {
  const loaded = loadedSource(() => ({ attempt: 4 }) as PlaybackSnapshot);
  loaded.load();
  expect(loaded.failOver({ kind: "http", status: 404, error: "Not Found" })).toEqual({
    type: "failOver",
    error: "Not Found",
    kind: "http",
    status: 404,
    attempt: 4,
  });
});

test("a video element's errors are sorted into what failed", () => {
  const error = (code: number) => ({ code, message: "MEDIA_ELEMENT_ERROR: Format error" });
  // A file this browser cannot play, or cannot decode, is the version's own.
  expect(mediaFailure(error(4)).kind).toBe("decode");
  expect(mediaFailure(error(3)).kind).toBe("decode");
  expect(mediaFailure(error(2)).kind).toBe("network");
  expect(mediaFailure(error(1))).toEqual({ kind: "other", error: "MEDIA_ELEMENT_ERROR: Format error" });
  expect(mediaFailure(null)).toEqual({ kind: "other", error: "player error" });
});

test("a failover starts the next source where the failed one stopped, a first start at the saved position", () => {
  const start = sourceStart();
  expect(start.next(30)).toBe(30);
  start.stop(312.5);
  expect(start.next(30)).toBe(312.5);
  expect(start.next(30)).toBe(30);
});

test("the playhead is the start until the element applied it", () => {
  const start = pendingStart();
  expect(start.position(0)).toBe(0);
  start.set(900);
  // A source that fails before its metadata stopped at its start, not at 0.
  expect(start.position(0)).toBe(900);
  expect(start.take()).toBe(900);
  expect(start.take()).toBeNull();
  expect(start.position(903.5)).toBe(903.5);
  start.set(0);
  expect(start.position(2)).toBe(2);
});

test("a seek before the metadata becomes the start, and none is pending after it", () => {
  const start = pendingStart();
  expect(start.retarget(30)).toBe(false);
  start.set(900);
  expect(start.retarget(1200)).toBe(true);
  // Progress reports and the metadata both see the viewer's position, not the old start.
  expect(start.position(0)).toBe(1200);
  expect(start.take()).toBe(1200);
  expect(start.retarget(5)).toBe(false);
});

test("an error before the metadata asks the source: a failed fetch is never decode", async () => {
  const error = { code: 4, message: "MEDIA_ELEMENT_ERROR: Format error" };
  const answering = (status: number) => async () => new Response("x", { status, statusText: status === 404 ? "Not Found" : "" });
  expect(await earlyFailure("http://127.0.0.1:1/s/t/f.mp4", error, answering(404))).toEqual({ kind: "http", status: 404, error: "Not Found" });
  expect(await earlyFailure("http://127.0.0.1:1/s/t/f.mp4", error, answering(502))).toMatchObject({ kind: "http", status: 502 });
  expect((await earlyFailure("u", error, async () => Promise.reject(new TypeError("Failed to fetch")))).kind).toBe("network");
  const late = async () => Promise.reject(new DOMException("The operation timed out.", "TimeoutError"));
  expect((await earlyFailure("u", error, late)).kind).toBe("timeout");
  // The bytes came and the browser still cannot play them: the file's own.
  expect((await earlyFailure("u", error, answering(206))).kind).toBe("decode");
});

test("Trying again shows while the core starts over, and a moment at least when it fails again at once", () => {
  const retry = { retries: 1, at: 1000 };
  // No Try again taken: nothing to say.
  expect(tryingAgain("resolving", null, 1000)).toBe(false);
  // Taken: while the core starts the play again.
  expect(tryingAgain("resolving", retry, 1200)).toBe(true);
  expect(tryingAgain("resolving", retry, 9000)).toBe(true);
  // Failed again at once: the same card as before, so the words hold a moment.
  expect(tryingAgain("failed", retry, 1200)).toBe(true);
  expect(tryingAgain("failed", retry, 2600)).toBe(false);
  // Started: nothing to say.
  expect(tryingAgain("ready", retry, 1200)).toBe(false);
});

test("an hls.js failure says whose it is", () => {
  const network = (details: string, code?: number) =>
    hlsFailure({ type: "networkError", details, ...(code === undefined ? {} : { response: { code } }) });
  expect(network("fragLoadError", 403)).toEqual({ kind: "http", status: 403, error: "hls.js fragLoadError" });
  expect(network("manifestLoadTimeOut")).toEqual({ kind: "timeout", error: "hls.js manifestLoadTimeOut" });
  expect(network("fragLoadError", 0)).toEqual({ kind: "network", error: "hls.js fragLoadError" });
  expect(hlsFailure({ type: "mediaError", details: "bufferAppendError" })).toEqual({ kind: "decode", error: "hls.js bufferAppendError" });
  expect(hlsFailure({ type: "otherError", details: "internalException" }).kind).toBe("other");
});
