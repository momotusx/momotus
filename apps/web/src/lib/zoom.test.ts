import { expect, test } from "bun:test";
import { browserZoom, desktopScale } from "./zoom";

test("desktopScale grows with the zoom instead of cancelling it, and holds its clamps", () => {
  // A window of 1440 CSS px at 100 percent, and the same window at 125 and 200 percent (1152 and 720 CSS px).
  expect(desktopScale(1440)).toBeCloseTo(1440 / 2300, 5);
  expect(desktopScale(1152, 1.25)).toBeCloseTo(desktopScale(1440), 5);
  // The page is a CSS px smaller and the zoom multiplies it back, so the text is 1.25 times bigger on screen.
  expect(desktopScale(1152, 1.25) * 1.25).toBeCloseTo(desktopScale(1440) * 1.25, 5);
  expect(desktopScale(720, 2) * 2).toBeCloseTo(desktopScale(1440) * 2, 5);
  // Clamps: small windows at 0.5, wide ones at 0.8.
  expect(desktopScale(800)).toBe(0.5);
  expect(desktopScale(3000)).toBe(0.8);
  expect(desktopScale(1500, 2)).toBe(0.8);
});

test("browserZoom reads a real browser zoom and refuses what only looks like one", () => {
  // No zoom, a missing outer width, or a ratio under 1.05 (borders): 1.
  expect(browserZoom(1440, 1440, 1)).toBe(1);
  expect(browserZoom(1440, 1425, 1)).toBe(1);
  expect(browserZoom(0, 1440, 1)).toBe(1);
  // A saved site zoom: 125 percent on a display at 200 percent (DPR 2.5), and on one at 125 percent (DPR 1.5625, borders).
  expect(browserZoom(1440, 1152, 2.5)).toBeCloseTo(1.25, 5);
  expect(browserZoom(1440, 1139, 1.5625)).toBeCloseTo(1.25, 5);
  // 200 percent on a plain display.
  expect(browserZoom(1440, 720, 2)).toBeCloseTo(2, 5);
  // Not a zoom: DPR 2 with a ratio of 1.28 matches no display scale, 1.333 matches one but 1.333 is no zoom step.
  expect(browserZoom(1440, 1125, 2)).toBe(1);
  expect(browserZoom(1440, 1080, 2)).toBe(1);
  // A docked DevTools on a plain display: the ratio 1.3 at DPR 1 is no display scale.
  expect(browserZoom(1440, 1108, 1)).toBe(1);
  // The zoom is held to 5, and Safari's 115 and 185 percent steps count on WebKit only.
  expect(browserZoom(1440, 288, 5)).toBe(5);
  expect(browserZoom(1440, 1252, 1.15)).toBe(1);
  expect(browserZoom(1440, 1252, 1.15, true)).toBeCloseTo(1.15, 5);
  expect(browserZoom(1440, 778, 1.85, true)).toBeCloseTo(1.85, 5);
});
