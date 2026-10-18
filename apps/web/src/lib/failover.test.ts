import { expect, test } from "bun:test";
import type { PlaybackSnapshot } from "../core/generated/PlaybackSnapshot";
import { loadedSource } from "./failover";

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
