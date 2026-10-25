/** The recorder's rules (e2e/replay.ts): what is kept, which names count as
 * never existing, what an unrecorded image becomes offline, and the stand-in
 * for the made-up debrid key. */

import { expect, test } from "@playwright/test";
import { keeps, madeUp, placeholder, standIn } from "./replay";

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

test("the specs' made-up Premiumize key is answered here as a working account, never asked live", () => {
  const check = "https://www.premiumize.me/api/cache/check?apikey=MQDUMMYE2EKEY1&items%5B%5D=a&items%5B%5D=b";
  const answer = standIn(check, undefined);
  expect(answer && "body" in answer && JSON.parse(Buffer.from(answer.body, "base64").toString())).toEqual({ status: "success", response: [false, false] });
  const other = standIn("https://www.premiumize.me/api/transfer/directdl", Buffer.from("apikey=MQDUMMYE2EKEY1B&src=x"));
  expect(other && "status" in other && other.status).toBe(504);
  expect(standIn("https://www.premiumize.me/api/cache/check?apikey=REALKEY&items%5B%5D=a", undefined)).toBeNull();
  expect(standIn("https://v3-cinemeta.strem.io/meta/movie/tt1.json?apikey=MQDUMMYE2EKEY1", undefined)).toBeNull();
});
