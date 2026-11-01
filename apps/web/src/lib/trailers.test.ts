import { expect, test } from "bun:test";
import { coverSize, heldStep, seekTarget, type TrailerSignals, trailerAllowed, youtubeId } from "./trailers";

const ok: TrailerSignals = { enabled: true, visible: true, reducedMotion: false, metered: false, walk: false, layout: "cover", capable: true, hasTrailer: true, incognito: false };

test("a preview plays only when every condition holds", () => {
  expect(trailerAllowed(ok)).toBe(true);
  const blocks: Partial<TrailerSignals>[] = [
    { enabled: false },
    { capable: false },
    { visible: false },
    { reducedMotion: true },
    { metered: true },
    { walk: true },
    { incognito: true },
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

test("a seek lands inside the trailer, short of the last 2 s where it closes", () => {
  expect(seekTarget(30, 150, 10)).toBe(40);
  expect(seekTarget(30, 150, -10)).toBe(20);
  expect(seekTarget(4, 150, -10)).toBe(0);
  expect(seekTarget(145, 150, 10)).toBe(148);
  expect(seekTarget(500, 150, 0)).toBe(148);
  expect(seekTarget(0, 1, 10)).toBe(0);
});

test("a held arrow's step grows as the film player's, held to a tenth of the trailer", () => {
  expect([0, 2, 3, 6, 10].map((r) => heldStep(r, 600))).toEqual([10, 10, 20, 30, 60]);
  expect([0, 3, 6, 10].map((r) => heldStep(r, 150))).toEqual([10, 15, 15, 15]);
  expect(heldStep(10, 40)).toBe(10);
});
