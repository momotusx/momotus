/** The recorder's rules (e2e/replay.ts): what is kept, which names count as
 * never existing, and what an unrecorded image becomes offline. */

import { expect, test } from "@playwright/test";
import { keeps, madeUp, placeholder } from "./replay";

test("the recorder keeps answers, never a server's failure or a 429", () => {
  for (const kept of [200, 206, 301, 404, 410, 499]) expect(keeps(kept), String(kept)).toBe(true);
  for (const dropped of [429, 500, 502, 503, 504]) expect(keeps(dropped), String(dropped)).toBe(false);
});

test("only a name that can never exist is kept as unknown", () => {
  for (const name of ["addon.example", "x.invalid", "a.b.test", "addon.example:8080"]) expect(madeUp(name), name).toBe(true);
  for (const name of ["v3-cinemeta.strem.io", "example.com", "images.metahub.space", "testing.dev"]) expect(madeUp(name), name).toBe(false);
});

test("an image missing offline is a grey placeholder; anything else is not", () => {
  const grey = placeholder("image/avif,image/webp,*/*");
  expect(grey?.subarray(1, 4).toString()).toBe("PNG");
  expect(placeholder("application/json")).toBeNull();
  expect(placeholder(undefined)).toBeNull();
});
