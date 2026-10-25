import { expect, test } from "bun:test";
import { type CueRects, cuePlace, cueSpot, SAFE_LINE } from "./cues";

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

test("two lines over a letterboxed wide film on a TV hang from a foot inside the picture, over the safe line", () => {
  // No controls block to measure the band against, only the safe line: the pure rule's fallback. The page has
  // `.osd` (hidden by opacity) from the first frame to the end, so this case is the rule's own, rarely the page's.
  const lines = drawn({ box: tv, video: wide, up: false, lines: two });
  expect(lines.fromFoot).toBe(true);
  expect(lines.bottom).toBeLessThanOrEqual(safeLine);
  expect(lines.bottom).toBeCloseTo(941.5 - 8, 5);
  expect(lines.top).toBeGreaterThanOrEqual(0);
  // The same with the hidden controls' block in the page.
  expect(drawn({ box: tv, video: wide, bar: { top: 870, bottom: 1026 }, up: false, lines: two }).bottom).toBeCloseTo(933.5, 5);
});

test("one line fits the band under the picture: drawn there, still over the safe line", () => {
  const one = drawn({ box: tv, video: wide, up: false, lines: two / 2 });
  expect(one.fromFoot).toBe(false);
  expect(one.top).toBeCloseTo(941.5 + 10, 5);
  expect(one.bottom).toBeLessThanOrEqual(safeLine);
});

test("a picture that fills the screen keeps its lines over the safe line, not 16 px from the foot", () => {
  const full = drawn({ box: tv, video: { width: 1920, height: 1080 }, up: false, lines: two });
  expect(full.fromFoot).toBe(true);
  expect(full.bottom).toBeCloseTo(safeLine, 5);
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

test("a phone upright draws under the picture, in the band over the controls, as before", () => {
  const phone = { top: 0, bottom: 844, width: 390, height: 844 };
  const lines = drawn({ box: phone, video: { width: 1920, height: 1080 }, bar: { top: 700, bottom: 820 }, up: false, lines: 2 * 24 * 1.3 });
  const pictureFoot = 844 - (844 - (1080 * 390) / 1920) / 2;
  expect(lines.fromFoot).toBe(false);
  expect(lines.top).toBeCloseTo(pictureFoot + 10, 5);
  expect(lines.bottom).toBeLessThanOrEqual(700);
});

const cue = { line: "auto" as number | "auto", snapToLines: true, position: "auto" as number | "auto", align: "center", size: 100 };

test("a cue with no line, or a line counted from the foot, joins the foot block", () => {
  expect(cueSpot(cue)).toBeNull();
  expect(cueSpot({ ...cue, line: -1 })).toBeNull();
  expect(cueSpot({ ...cue, line: -2 })).toBeNull();
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
