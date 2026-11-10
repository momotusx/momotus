import { expect, test } from "bun:test";
import { type CueRects, cuePlace, cueSpot, cueTop, FOOT, SAFE_LINE } from "./cues";

/** A 1920x1080 TV, the element filling it. */
const tv = { top: 0, bottom: 1080, width: 1920, height: 1080 };
/** A 2.39:1 film: 803 px tall at the TV's width, 138.5 px bands. */
const wide = { width: 1920, height: 803 };
/** Two lines at 44 px, 1.3 a line. */
const two = 2 * 44 * 1.3;

/** The lines' box from the top of the element, as the page draws it. */
function drawn(rects: CueRects) {
  const { at, shrink } = cuePlace(rects);
  const height = rects.lines * (shrink ? 0.85 : 1);
  const top = at.top !== undefined ? rects.box.top + at.top : rects.box.bottom - (at.bottom ?? 0) - height;
  return { top, bottom: top + height, fromFoot: at.bottom !== undefined, shrink };
}

const safeLine = tv.bottom - tv.height * SAFE_LINE;

/** The picture's foot and height on the TV under a 2.39:1 film. */
const wideFoot = 1080 - (1080 - 803) / 2;
const wideHeight = 803;

test("two lines over a letterboxed wide film on a TV hang 8% of the picture over its foot, over the safe line", () => {
  // No controls block in the rects: the pure foot. The page has `.osd` (hidden by opacity) from the first frame to the
  // end; a hidden block changes nothing on a landscape screen, as the second case shows.
  const lines = drawn({ box: tv, video: wide, up: false, lines: two });
  expect(lines.fromFoot).toBe(true);
  expect(lines.bottom).toBeCloseTo(wideFoot - wideHeight * FOOT, 5);
  expect(lines.bottom).toBeLessThanOrEqual(safeLine);
  expect(lines.top).toBeGreaterThanOrEqual(0);
  // The same with the hidden controls' block in the page.
  expect(drawn({ box: tv, video: wide, bar: { top: 870, bottom: 1026 }, up: false, lines: two }).bottom).toBeCloseTo(wideFoot - wideHeight * FOOT, 5);
});

test("one line on a landscape TV hangs from the same foot as two, never in the band (no jump between cues)", () => {
  const one = drawn({ box: tv, video: wide, up: false, lines: two / 2 });
  const both = drawn({ box: tv, video: wide, up: false, lines: two });
  expect(one.fromFoot).toBe(true);
  expect(one.bottom).toBeCloseTo(both.bottom, 5);
  expect(one.bottom).toBeLessThan(wideFoot);
});

test("a picture that fills the screen hangs its lines 8% of its height over its foot, over the safe line", () => {
  const full = drawn({ box: tv, video: { width: 1920, height: 1080 }, up: false, lines: two });
  expect(full.fromFoot).toBe(true);
  expect(full.bottom).toBeCloseTo(1080 - 1080 * FOOT, 5);
  // A landscape phone (844x390, 16:9 pillarboxed): 8% of 390, 31.2 px.
  const phone = { top: 0, bottom: 390, width: 844, height: 390 };
  expect(drawn({ box: phone, video: { width: 1920, height: 1080 }, up: false, lines: 2 * 24 * 1.3 }).bottom).toBeCloseTo(390 - 390 * FOOT, 5);
  // The video's size not known yet: the element's own height stands for the picture.
  expect(drawn({ box: tv, video: { width: 0, height: 0 }, up: false, lines: two }).bottom).toBeCloseTo(1080 - 1080 * FOOT, 5);
});

test("an element that does not start at the page's top keeps the same foot from its own foot", () => {
  const desktop = { top: 100, bottom: 910, width: 1440, height: 810 };
  const picture = 1440 / 2.39;
  const lines = drawn({ box: desktop, video: { width: 1920, height: 1920 / 2.39 }, up: false, lines: two });
  expect(lines.bottom).toBeCloseTo(910 - (810 - picture) / 2 - picture * FOOT, 5);
});

test("the controls up: 8 px over the scrubber, 85% while they would meet the transport", () => {
  const scrubber = { top: 900, bottom: 908 };
  const up = drawn({ box: tv, video: { width: 1920, height: 1080 }, bar: { top: 860, bottom: 1026 }, scrubber, up: true, lines: two });
  expect(up.bottom).toBeCloseTo(892, 5);
  expect(up.shrink).toBe(false);
  const near = drawn({ box: tv, video: { width: 1920, height: 1080 }, scrubber, transport: { top: 700, bottom: 800 }, up: true, lines: two });
  expect(near.bottom).toBeCloseTo(892, 5);
  expect(near.shrink).toBe(true);
});

test("a wide film with the controls up: the higher of its 8% foot and 8 px over the scrubber", () => {
  const rects = { box: tv, video: wide, bar: { top: 860, bottom: 1026 }, transport: { top: 486, bottom: 594 }, up: true, lines: two };
  // A low scrubber (900): the picture's own foot, 877.26, is the higher.
  const low = drawn({ ...rects, scrubber: { top: 900, bottom: 908 } });
  expect(low.bottom).toBeCloseTo(wideFoot - wideHeight * FOOT, 5);
  expect(low.shrink).toBe(false);
  // A high one (850): 8 px over it.
  expect(drawn({ ...rects, scrubber: { top: 850, bottom: 858 } }).bottom).toBeCloseTo(842, 5);
});

test("the position moves the hidden-controls foot: 5, 8 and 25 percent of the picture, never under the 5% line", () => {
  const full = { width: 1920, height: 1080 };
  const at = (position: number, video = full) => drawn({ box: tv, video, up: false, lines: two, position }).bottom;
  expect(at(8)).toBeCloseTo(1080 - 1080 * 0.08, 5);
  expect(at(25)).toBeCloseTo(1080 - 1080 * 0.25, 5);
  // 5 percent of a full picture is the safe line itself.
  expect(at(5)).toBeCloseTo(Math.min(1080 - 1080 * 0.05, safeLine), 5);
  // A wide film: 5 percent over its foot, the band under it, is over the safe line.
  expect(at(5, wide)).toBeCloseTo(wideFoot - wideHeight * 0.05, 5);
  expect(at(25, wide)).toBeCloseTo(wideFoot - wideHeight * 0.25, 5);
  // No position given: the 8% default.
  expect(drawn({ box: tv, video: full, up: false, lines: two }).bottom).toBeCloseTo(at(8), 5);
});

test("the controls up: the lifted foot plus the position's share over 8, and 8 px over an open strip", () => {
  const full = { width: 1920, height: 1080 };
  const scrubber = { top: 900, bottom: 908 };
  const at = (position: number, strip?: { top: number; bottom: number }) => drawn({ box: tv, video: full, scrubber, strip, up: true, lines: two, position }).bottom;
  expect(at(8)).toBeCloseTo(892, 5);
  // 5 is lower than 8: the lift holds, the line never goes down onto the bar.
  expect(at(5)).toBeCloseTo(892, 5);
  expect(at(12)).toBeCloseTo(892 - 1080 * 0.04, 5);
  expect(at(25)).toBeCloseTo(892 - 1080 * 0.17, 5);
  // A strip open over the bar: its top, 8 px clear, then the position's share.
  expect(at(8, { top: 800, bottom: 848 })).toBeCloseTo(792, 5);
  expect(at(12, { top: 800, bottom: 848 })).toBeCloseTo(792 - 1080 * 0.04, 5);
});

test("a phone across with the strip and Style open hangs the lines over the transport, not under it", () => {
  // Pixel walk 2026-11-09 L23 at 844 x 390: the Style row's top at 220 lifted the foot to 212, inside the transport
  // (164 to 226), and the lines landed under Back 10, Play and Forward 10.
  const phone = { top: 0, bottom: 390, width: 844, height: 390 };
  const video = { width: 1920, height: 1080 };
  const transport = { top: 164, bottom: 226 };
  const lines = 2 * 24 * 1.3;
  const open = drawn({ box: phone, video, scrubber: { top: 350, bottom: 358 }, transport, strip: { top: 220, bottom: 330 }, up: true, lines });
  expect(open.bottom).toBeCloseTo(transport.top - 8, 5);
  expect(open.shrink).toBe(false);
  // No strip: the old rule, 8 px over the scrubber, 85% when it would meet the transport.
  const closed = drawn({ box: phone, video, scrubber: { top: 350, bottom: 358 }, transport, up: true, lines });
  expect(closed.bottom).toBeCloseTo(342, 5);
});

test("a phone upright keeps the band whatever the position", () => {
  const phone = { top: 0, bottom: 844, width: 390, height: 844 };
  const pictureFoot = 844 - (844 - (1080 * 390) / 1920) / 2;
  for (const position of [5, 8, 25]) {
    const lines = drawn({ box: phone, video: { width: 1920, height: 1080 }, bar: { top: 700, bottom: 820 }, up: false, lines: 2 * 24 * 1.3, position });
    expect(lines.fromFoot).toBe(false);
    expect(lines.top).toBeCloseTo(pictureFoot + 10, 5);
  }
});

test("a phone upright draws under the picture, in the band over the controls, as before", () => {
  const phone = { top: 0, bottom: 844, width: 390, height: 844 };
  const lines = drawn({ box: phone, video: { width: 1920, height: 1080 }, bar: { top: 700, bottom: 820 }, up: false, lines: 2 * 24 * 1.3 });
  const pictureFoot = 844 - (844 - (1080 * 390) / 1920) / 2;
  expect(lines.fromFoot).toBe(false);
  expect(lines.top).toBeCloseTo(pictureFoot + 10, 5);
  expect(lines.bottom).toBeLessThanOrEqual(700);
});

test("a phone upright with more lines than its band holds hangs them 8% of the picture over its foot", () => {
  const phone = { top: 0, bottom: 844, width: 390, height: 844 };
  const picture = (1080 * 390) / 1920;
  const pictureFoot = 844 - (844 - picture) / 2;
  // Four Large lines: 4 x 24 x 1.3 = 124.8 px, more than the band over the controls holds (640 - 531.7 - 10).
  const lines = drawn({ box: phone, video: { width: 1920, height: 1080 }, bar: { top: 640, bottom: 820 }, up: false, lines: 4 * 24 * 1.3 });
  expect(lines.fromFoot).toBe(true);
  expect(lines.bottom).toBeCloseTo(pictureFoot - picture * FOOT, 5);
});

const cue = { line: "auto" as number | "auto", snapToLines: true, position: "auto" as number | "auto", align: "center", size: 100 };

test("a cue with no line, or a line counted from the foot, joins the foot block", () => {
  expect(cueSpot(cue)).toBeNull();
  expect(cueSpot({ ...cue, line: -1 })).toBeNull();
  expect(cueSpot({ ...cue, line: -2 })).toBeNull();
  // A line at 90% or lower is a foot line (some converters write line:90% for dialogue): the foot rule places it.
  expect(cueSpot({ ...cue, line: 90, snapToLines: false })).toBeNull();
  expect(cueSpot({ ...cue, line: 95, snapToLines: false })).toBeNull();
  expect(cueSpot({ ...cue, line: 89, snapToLines: false })?.top).toBe("89%");
  // A position or an align of its own does not take it out of the foot block.
  expect(cueSpot({ ...cue, position: 20, align: "start" })).toBeNull();
});

test("a cue its file placed stays where the file put it", () => {
  // A sign at line:10% (an SSA \an8 made WebVTT), centred, the whole width.
  expect(cueSpot({ ...cue, line: 10, snapToLines: false })).toEqual({ top: "10%", down: 0, left: "0%", width: "100%", align: "center" });
  // Line 2 from the top, in lines of its own height.
  expect(cueSpot({ ...cue, line: 2 })).toMatchObject({ top: "0%", down: 2 });
  // 40% wide at position 30%, its box's start there, the text at its start.
  expect(cueSpot({ ...cue, line: 0, position: 30, align: "start", size: 40 })).toEqual({ top: "0%", down: 0, left: "30%", width: "40%", align: "flex-start" });
  // Centred on 90%, 40% wide: kept inside the element.
  expect(cueSpot({ ...cue, line: 0, position: 90, size: 40 })?.left).toBe("60%");
  // End-aligned with no position: its box ends at the element's end.
  expect(cueSpot({ ...cue, line: 0, align: "right", size: 40 })).toEqual({ top: "0%", down: 0, left: "60%", width: "40%", align: "flex-end" });
});

/** Split's second line (plans/dual-subtitles.md 4 B): the mirror of the foot. */
test("Split hangs the second line 8% of the picture under the picture's top, over the 5% line", () => {
  const rects = { box: tv, video: wide, up: false, lines: two, signs: [] };
  const pictureTop = (1080 - 803) / 2;
  expect(cueTop(rects)).toBeCloseTo(pictureTop + 803 * FOOT, 5);
  // A picture that fills the screen: 8% of it, never over the 5% line.
  expect(cueTop({ ...rects, video: { width: 1920, height: 1080 } })).toBeCloseTo(1080 * FOOT, 5);
  // An element under the page's top counts from its own top.
  const desktop = { top: 100, bottom: 910, width: 1440, height: 810 };
  const picture = 1440 / 2.39;
  expect(cueTop({ ...rects, box: desktop, video: { width: 1920, height: 1920 / 2.39 } })).toBeCloseTo((810 - picture) / 2 + picture * FOOT, 5);
});

test("Split's second line goes under the top bar while the controls show, and under a sign in the top half", () => {
  const rects = { box: tv, video: { width: 1920, height: 1080 }, lines: two, signs: [] };
  expect(cueTop({ ...rects, up: true, top: { top: 40, bottom: 160 } })).toBeCloseTo(168, 5);
  // Hidden controls: the bar does not count.
  expect(cueTop({ ...rects, up: false, top: { top: 40, bottom: 160 } })).toBeCloseTo(1080 * FOOT, 5);
  // A sign at the top pushes it under; one in the lower half does not.
  expect(cueTop({ ...rects, up: false, signs: [{ top: 60, bottom: 200 }] })).toBeCloseTo(208, 5);
  expect(cueTop({ ...rects, up: false, signs: [{ top: 700, bottom: 800 }] })).toBeCloseTo(1080 * FOOT, 5);
  // An element under the page's top: the bar, the signs and the half are all counted from its own top.
  const low = { ...rects, box: { top: 100, bottom: 1180, width: 1920, height: 1080 } };
  expect(cueTop({ ...low, up: true, top: { top: 140, bottom: 260 } })).toBeCloseTo(168, 5);
  expect(cueTop({ ...low, up: false, signs: [{ top: 160, bottom: 300 }] })).toBeCloseTo(208, 5);
  // Under the element's half (the page's 640): not a top sign.
  expect(cueTop({ ...low, up: false, signs: [{ top: 660, bottom: 760 }] })).toBeCloseTo(1080 * FOOT, 5);
});

test("Split on an upright phone takes the band over the picture when it holds the line", () => {
  const phone = { top: 0, bottom: 844, width: 390, height: 844 };
  const picture = (1080 * 390) / 1920;
  const pictureTop = (844 - picture) / 2;
  const one = 24 * 1.3;
  expect(cueTop({ box: phone, video: { width: 1920, height: 1080 }, up: false, lines: one, signs: [] })).toBeCloseTo(pictureTop - one - 10, 5);
  // A block the band cannot hold hangs inside the picture.
  expect(cueTop({ box: phone, video: { width: 1920, height: 1080 }, up: false, lines: 400, signs: [] })).toBeCloseTo(pictureTop + picture * FOOT, 5);
  // The same phone 50 px down the page: the same place in it.
  const lower = { ...phone, top: 50, bottom: 894 };
  expect(cueTop({ box: lower, video: { width: 1920, height: 1080 }, up: false, lines: one, signs: [] })).toBeCloseTo(pictureTop - one - 10, 5);
});
