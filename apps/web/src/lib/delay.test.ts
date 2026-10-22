import { expect, test } from "bun:test";
import { delayText, delayWords, holdStep, MAX_DELAY, nudge } from "./delay";

test("a held press speeds up as Stremio's: 0.3 s a tick, then 0.4, 0.5 and 0.6 from the third second", () => {
  expect(holdStep(250)).toBeCloseTo(0.3, 10);
  expect(holdStep(999)).toBeCloseTo(0.3, 10);
  expect(holdStep(1000)).toBeCloseTo(0.4, 10);
  expect(holdStep(2500)).toBeCloseTo(0.5, 10);
  expect(holdStep(3000)).toBeCloseTo(0.6, 10);
  expect(holdStep(60_000)).toBeCloseTo(0.6, 10);
});

test("a step lands on a tenth and stays within 60 s either way", () => {
  // 0.2 + 0.1 is 0.30000000000000004 in floating point: the value is the tenth.
  expect(nudge(0.2, 0.1)).toBe(0.3);
  expect(nudge(0.1, -0.1)).toBe(0);
  expect(Object.is(nudge(0.1, -0.1), -0)).toBe(false);
  expect(nudge(-1.1, -0.6)).toBe(-1.7);
  expect(nudge(59.9, 0.6)).toBe(MAX_DELAY);
  expect(nudge(-59.9, -0.6)).toBe(-MAX_DELAY);
  expect(nudge(60, 0.1)).toBe(60);
});

test("the value shows its sign and one decimal, and reads in words", () => {
  expect(delayText(0)).toBe("0.0 s");
  expect(delayText(0.3)).toBe("+0.3 s");
  expect(delayText(-1.2)).toBe("−1.2 s");
  expect(delayText(60)).toBe("+60.0 s");
  expect(delayWords(0)).toBe("Subtitles in step");
  expect(delayWords(0.3)).toBe("Subtitles 0.3 seconds later");
  expect(delayWords(-1.2)).toBe("Subtitles 1.2 seconds earlier");
});
