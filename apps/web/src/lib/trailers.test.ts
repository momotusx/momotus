import { expect, test } from "bun:test";
import { coverSize, type TrailerSignals, trailerAllowed, youtubeId } from "./trailers";

const ok: TrailerSignals = { enabled: true, visible: true, reducedMotion: false, metered: false, walk: false, layout: "cover", capable: true, hasTrailer: true };

test("a preview plays only when every condition holds", () => {
  expect(trailerAllowed(ok)).toBe(true);
  const blocks: Partial<TrailerSignals>[] = [
    { enabled: false },
    { capable: false },
    { visible: false },
    { reducedMotion: true },
    { metered: true },
    { walk: true },
    { layout: "none" },
    { hasTrailer: false },
  ];
  for (const b of blocks) expect([b, trailerAllowed({ ...ok, ...b })]).toEqual([b, false]);
});

test("only YouTube's own id shape goes into a player", () => {
  expect(youtubeId("cdx31ak4KbQ")).toBe("cdx31ak4KbQ");
  expect(youtubeId("cdx31ak4KbQ&list=x")).toBeNull();
  expect(youtubeId('cdx31ak4"><')).toBeNull();
  expect(youtubeId(null)).toBeNull();
});

test("the player covers the frame past every edge, 16:9", () => {
  const phone = coverSize(390, 680);
  expect(phone.height).toBeCloseTo(680 * 1.12, 5);
  expect(phone.width / phone.height).toBeCloseTo(16 / 9, 5);
  expect(phone.width).toBeGreaterThan(390);
  const wide = coverSize(1440, 600);
  expect(wide.width).toBeGreaterThan(1440);
  expect(wide.height).toBeGreaterThan(600);
});
