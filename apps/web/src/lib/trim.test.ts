import { expect, test } from "bun:test";
import { alphaBox } from "./trim";

/** A w x h RGBA image, transparent except the listed pixels. */
function image(w: number, h: number, opaque: [number, number][], alpha = 255) {
  const data = new Uint8ClampedArray(w * h * 4);
  for (const [x, y] of opaque) data[(y * w + x) * 4 + 3] = alpha;
  return data;
}

test("finds the opaque box inside transparent margins", () => {
  expect(alphaBox(image(10, 6, [[3, 1], [7, 4], [5, 2]]), 10, 6)).toEqual({ left: 3, top: 1, right: 7, bottom: 4 });
});

test("an image with no opaque pixel has no box; faint pixels do not count", () => {
  expect(alphaBox(image(4, 4, []), 4, 4)).toBeNull();
  expect(alphaBox(image(4, 4, [[1, 1]], 6), 4, 4)).toBeNull();
});

test("a full image is its own box", () => {
  const all: [number, number][] = [];
  for (let y = 0; y < 3; y++) for (let x = 0; x < 5; x++) all.push([x, y]);
  expect(alphaBox(image(5, 3, all), 5, 3)).toEqual({ left: 0, top: 0, right: 4, bottom: 2 });
});
