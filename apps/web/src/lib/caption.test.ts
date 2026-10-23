import { expect, test } from "bun:test";
import type { Tile } from "../core/generated/Tile";
import { caption } from "./words";

const tile = (t: Partial<Tile>): Tile => ({
  id: "tt1",
  kind: "series",
  name: "Show",
  poster: null,
  posterFallback: null,
  background: null,
  logo: null,
  posterShape: "poster",
  releaseInfo: null,
  progress: null,
  videoId: null,
  rating: null,
  ratings: { imdb: null },
  ...t,
});

test("Continue Watching shows the core's caption as is; catalog tiles show none", () => {
  expect(caption(tile({ progress: 0.37, videoId: "tt1:2:3", releaseInfo: "S2:E3 · 36 min left" }))).toBe("S2:E3 · 36 min left");
  expect(caption(tile({ kind: "movie", progress: 0.24, releaseInfo: "1 h 12 min left" }))).toBe("1 h 12 min left");
  expect(caption(tile({ releaseInfo: "Next: S1:E2", videoId: "tt1:1:2" }))).toBe("Next: S1:E2");
  expect(caption(tile({ releaseInfo: "2011–2019" }))).toBeNull();
});
