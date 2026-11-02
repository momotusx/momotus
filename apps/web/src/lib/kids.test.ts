import { expect, test } from "bun:test";
import type { ProfilesSnapshot } from "../core/generated/ProfilesSnapshot";
import { KIDS_HIDDEN_SECTIONS, kidsBlocked, kidsSession, kidsWords } from "./kids";

const snap = (kids: boolean, profileKids = kids) =>
  ({ active: "k", kids, profiles: [{ id: "k", name: "Kid", kids: profileKids }] }) as unknown as ProfilesSnapshot;

test("a kids session is the core's flag or the active profile's own", () => {
  expect(kidsSession(snap(true, false))).toBe(true);
  expect(kidsSession(snap(false, true))).toBe(true);
  expect(kidsSession(snap(false))).toBe(false);
  expect(kidsSession(null)).toBe(false);
});

test("a kids session opens no Discover, Calendar, source, account or profile page (QA r1 Q7 rows 27, 32)", () => {
  for (const path of ["/discover", "/calendar", "/sign-in", "/sign-in/email", "/sync", "/sync/start", "/debrid", "/trakt", "/import", "/find-addons", "/recommended-addons", "/addons", "/plugins", "/pair", "/welcome", "/home-rows", "/profiles/new", "/profiles/manage", "/settings/sources", "/settings/account"]) {
    expect(kidsBlocked(path)).toBe(true);
  }
  for (const path of ["/", "/search", "/library", "/library/downloads", "/profiles", "/settings", "/settings/playback", "/settings/subtitles", "/title/movie/tt1", "/play/movie/tt1/tt1", "/stream-choice"]) {
    expect(kidsBlocked(path)).toBe(false);
  }
  expect(KIDS_HIDDEN_SECTIONS).toEqual(["sources", "account", "home", "downloads"]);
});

test("a kids search speaks of its list, never of addons", () => {
  expect(kidsWords.searchNone).toBe("That isn't on your list. Ask a grown-up.");
  expect(Object.values(kidsWords).join(" ")).not.toMatch(/addon/i);
});
