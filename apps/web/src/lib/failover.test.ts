import { expect, test } from "bun:test";
import type { PlaybackSnapshot } from "../core/generated/PlaybackSnapshot";
import { loadedSource, sourceStart } from "./failover";

test("a failover names the source the engine loaded, not the one the core moved to", () => {
  let snapshot = { attempt: 1 } as PlaybackSnapshot;
  const loaded = loadedSource(() => snapshot);
  loaded.load();
  snapshot = { attempt: 2 } as PlaybackSnapshot;
  expect(loaded.failOver("decoder error")).toEqual({ type: "failOver", error: "decoder error", attempt: 1 });
});

test("with no source loaded the command names none", () => {
  const loaded = loadedSource(() => null);
  expect(loaded.failOver("decoder error")).toEqual({ type: "failOver", error: "decoder error" });
  loaded.load();
  expect(loaded.failOver("decoder error")).toEqual({ type: "failOver", error: "decoder error" });
});

test("a failover starts the next source where the failed one stopped, a first start at the saved position", () => {
  const start = sourceStart();
  expect(start.next(30)).toBe(30);
  start.stop(312.5);
  expect(start.next(30)).toBe(312.5);
  expect(start.next(30)).toBe(30);
});
