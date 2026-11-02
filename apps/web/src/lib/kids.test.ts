import { expect, test } from "bun:test";
import type { KidsKeysSnapshot } from "../core/generated/KidsKeysSnapshot";
import type { ProfileInfo } from "../core/generated/ProfileInfo";
import type { ProfilesSnapshot } from "../core/generated/ProfilesSnapshot";
import { allowProblem, KIDS_HIDDEN_SECTIONS, kidsBlocked, kidsChoices, kidsSession, kidsWords, managesKids, ownerOf, pinAsked, takenOffHere, titleRefused, writePolicy } from "./kids";
import { kidsCopy, kidsLine } from "./words";

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

test("a title the core refused for a kids profile is said at once (QA r1 Q7 row 28)", () => {
  expect(titleRefused({ state: "notAllowed" })).toBe(true);
  expect(titleRefused({ state: "loading" })).toBe(false);
  expect(titleRefused({ state: "failed", reason: "x" })).toBe(false);
  expect(titleRefused(undefined)).toBe(false);
  expect(`${kidsWords.titleRefused}. ${kidsWords.ask}`).toBe("That isn't on your list. Ask a grown-up.");
});

// The owner's side (plans/m11-kids.md 6 to 8).

const main: ProfileInfo = { id: "default", name: "Main", avatar: "letter:ember", position: 0, hasPin: true, isOwner: true, syncing: false, kids: false };
const kid: ProfileInfo = { id: "k", name: "Kid", avatar: "letter:sky", position: 1, hasPin: false, isOwner: false, syncing: false, kids: true };
const sam: ProfileInfo = { id: "s", name: "Sam", avatar: "letter:sea", position: 2, hasPin: false, isOwner: false, syncing: false, kids: false };
const session = (active: string, over: Partial<ProfilesSnapshot> = {}, profiles = [main, kid, sam]) =>
  ({ profiles, active, locked: false, max: 6, kids: active === "k", wipePending: null, incognitoHost: null, incognitoPin: null, incognitoEnded: false, ...over }) as ProfilesSnapshot;

test("only the main profile's unlocked session manages kids profiles", () => {
  expect(managesKids(session("default"))).toBe(true);
  expect(managesKids(session("default", { locked: true }))).toBe(false);
  expect(managesKids(session("s"))).toBe(false);
  expect(managesKids(session("k"))).toBe(false);
  // A session that ran as a kids profile until its next switch stays out, whoever it shows now.
  expect(managesKids(session("default", { kids: true }))).toBe(false);
  expect(managesKids(null)).toBe(false);
});

test("leaving a kids profile asks the main profile's PIN for any profile, never the target's (plans/m11-kids.md 7)", () => {
  expect(pinAsked(session("k"), sam)).toBe("main");
  expect(pinAsked(session("k"), main)).toBe("main");
  expect(pinAsked(session("k"), { ...sam, hasPin: true })).toBe("main");
  // The kids profile itself is free, locked or not.
  expect(pinAsked(session("k"), kid)).toBe(null);
  // No main PIN: nothing to type, the core refuses the exit (kidsNeedPin) and the picker says where to set one.
  expect(pinAsked(session("k", {}, [{ ...main, hasPin: false }, kid, sam]), sam)).toBe(null);
  // Anyone else asks the target's own PIN, and entering a kids profile is free.
  expect(pinAsked(session("default"), { ...sam, hasPin: true })).toBe("own");
  expect(pinAsked(session("default"), sam)).toBe(null);
  expect(pinAsked(session("default"), kid)).toBe(null);
  expect(ownerOf(session("k"))?.id).toBe("default");
});

test("allowed or not comes from the core's kidsKeys, per kids profile in its order", () => {
  const keys: KidsKeysSnapshot = { profiles: [{ profile: "k", name: "Kid", keys: ["movie:tt1"] }, { profile: "k2", name: "Ada", keys: ["series:tt1"] }] };
  expect(kidsChoices(keys, "movie", "tt1")).toEqual([
    { profile: "k", name: "Kid", allowed: true },
    { profile: "k2", name: "Ada", allowed: false },
  ]);
  expect(kidsChoices(null, "movie", "tt1")).toEqual([]);
});

test("an Allow the core refused says why in the plan's words; other failures keep their own", () => {
  expect(allowProblem("mismatch", "Kid")).toBe("This title's details came under another id. Open it from another row to allow it.");
  expect(allowProblem("noDetails", "Kid")).toBe("Momotus could not read this title's details. Try again.");
  expect(allowProblem("listFull", "Kid")).toBe("Kid's list is full (1,000 titles). Remove some first.");
  expect(allowProblem("network", "Kid")).toBe(null);
  expect(allowProblem(undefined, "Kid")).toBe(null);
});

test("a title taken off the list closes its own page and player, nothing else (plans/m11-kids.md 8)", () => {
  expect(takenOffHere("/title/movie/tt1", ["tt1"])).toBe(true);
  expect(takenOffHere("/title/series/tt2/versions/tt2:1:1", ["tt2"])).toBe(true);
  expect(takenOffHere("/play/series/tt2/tt2%3A1%3A1", ["tt2"])).toBe(true);
  expect(takenOffHere("/title/movie/tt1", ["tt9"])).toBe(false);
  expect(takenOffHere("/", ["tt1"])).toBe(false);
  expect(takenOffHere("/library", ["tt1"])).toBe(false);
});

test("a policy write syncs at once when an account is joined, and only after it was written", async () => {
  const order: string[] = [];
  let joined = true;
  const sync = { joined: () => joined, now: () => order.push("sync") };
  expect(await writePolicy(async () => (order.push("write"), 7), sync)).toBe(7);
  expect(order).toEqual(["write", "sync"]);
  joined = false;
  await writePolicy(async () => order.push("write"), sync);
  expect(order).toEqual(["write", "sync", "write"]);
  joined = true;
  await expect(writePolicy(() => Promise.reject(new Error("refused")), sync)).rejects.toThrow("refused");
  expect(order).toEqual(["write", "sync", "write"]);
});

test("the kids copy is the plan's (6.4), filled, with no dash, no addon name and no raw code", () => {
  expect(kidsLine("allow", { name: "Kid" })).toBe("Allow for Kid");
  expect(kidsLine("allowed", { name: "Kid" })).toBe("Allowed for Kid");
  expect(kidsLine("allowedSpoken", { name: "Kid" })).toBe("Allowed for Kid, select to remove");
  expect(kidsLine("remove", { name: "Kid" })).toBe("Remove from Kid's list");
  expect(kidsLine("allowing")).toBe("Allowing…");
  expect(kidsLine("sheetTitle")).toBe("Who can watch this?");
  expect(kidsLine("addTitle", { name: "Kid" })).toBe("Add titles for Kid");
  expect(kidsLine("listHeading", { name: "Kid" })).toBe("What Kid can watch");
  expect(kidsLine("viewAs", { name: "Kid" })).toBe("View as Kid");
  expect(kidsLine("empty", { name: "Kid" })).toBe("Nothing yet. Choose Add titles, or Allow for Kid on any show or movie.");
  expect(kidsLine("makeStandardAsk", { name: "Kid" })).toBe("Make Kid a standard profile?");
  expect(kidsLine("makeStandardBody", { name: "Kid" })).toBe("Kid will see everything your addons show and can leave without your PIN.");
  expect([kidsLine("keepKids"), kidsLine("makeStandardYes")]).toEqual(["Keep it a kids profile", "Make standard"]);
  expect(kidsLine("switchNeedsPin", { owner: "Main" })).toBe("Set a PIN on Main first. A kids profile is only as safe as that PIN.");
  expect(kidsLine("exitTitle", { owner: "Main" })).toBe("Ask a grown-up to enter Main's PIN");
  expect(kidsLine("exitWaitNote", { owner: "Main" })).toBe("Or set a new PIN for Main on another device.");
  expect(kidsLine("exitNoPin", { owner: "Main" })).toBe("Main has no PIN. Set one on your phone or TV, then try again.");
  expect(kidsLine("titleMany", { n: 1000 })).toBe("1,000 titles");
  expect(kidsLine("takenOff")).toBe("A grown-up took this off your list.");
  for (const line of Object.values(kidsCopy)) {
    expect(line).not.toMatch(/\u2014|\w--\w|cinemeta|torrentio|opensubtitles|https?:|\b[45]\d\d\b/i);
  }
});
