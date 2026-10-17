import { expect, test } from "bun:test";
import type { Tile } from "../core/generated/Tile";
import { caption } from "./words";

const tile = (t: Partial<Tile>): Tile => ({
  id: "tt1",
  kind: "series",
  name: "Show",
  poster: null,
  background: null,
  logo: null,
  posterShape: "poster",
  releaseInfo: null,
  progress: null,
  videoId: null,
  ...t,
});

test("captions say where you are without guessing the kind", () => {
  expect(caption(tile({ progress: 0.37, videoId: "tt1:2:3" }))).toBe("S2:E3, 37% watched");
  expect(caption(tile({ kind: "movie", progress: 0.24, videoId: "tt9" }))).toBe("Movie, 24% watched");
  expect(caption(tile({ progress: 0.37 }))).toBe("37% watched");
  expect(caption(tile({ releaseInfo: "Next: S1 E2", videoId: "tt1:1:2" }))).toBe("Next: S1 E2");
  expect(caption(tile({}))).toBeNull();
});
