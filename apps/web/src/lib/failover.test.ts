import { expect, test } from "bun:test";
import type { PlaybackSnapshot } from "../core/generated/PlaybackSnapshot";
import { loadedSource, mediaFailure, pendingStart, sourceStart } from "./failover";

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
  start.applied();
  expect(start.position(903.5)).toBe(903.5);
  start.set(0);
  expect(start.position(2)).toBe(2);
});
