import { expect, test } from "bun:test";
import shared from "../../../shared/trakt-words.json";
import type { TraktLoginSnapshot } from "../core/generated/TraktLoginSnapshot";
import type { TraktStatus } from "../core/generated/TraktStatus";
import { fill, rowLine, loginLine, loginRetry, newer, pendingLine, playbackMark, stateCard, timeLeft, type TraktRecord, TraktRecords, traktWords } from "./trakt";

const base = { user: "ana", lastOkMs: 0, lastError: null, pendingWrites: 0, unmapped: [], importing: null, record: null };
const clock = () => "2:05 PM";
const status = (state: Record<string, unknown>) => ({ ...base, ...state }) as unknown as TraktStatus;

test("each state says its own words and offers its one way on", () => {
  const cards = [
    stateCard(status({ state: "connected" }), clock),
    stateCard(status({ state: "failing", sinceMs: 1, reason: "status 503" }), clock),
    stateCard(status({ state: "reconnect", reason: "the sign-in was revoked" }), clock),
    stateCard(status({ state: "off" }), clock),
    stateCard(status({ state: "thisDeviceOff" }), clock),
    stateCard(status({ state: "notConfigured" }), clock),
  ];
  expect(new Set(cards.map((c) => c.line)).size).toBe(6);
  expect(cards[0]).toEqual({ line: "Connected as ana", actions: ["syncNow", "signOut"] });
  expect(cards[1]?.line).toContain("since 2:05 PM: status 503");
  expect(stateCard(status({ state: "failing", sinceMs: 1, reason: "x", lastError: "status 503" }), clock).detail).toBe("Last error: status 503");
  expect(stateCard(status({ state: "connected", user: null }), clock).line).toBe(traktWords.connectedNoUser);
  expect(cards[2]?.line).toContain("the sign-in was revoked");
  expect(cards[2]?.actions).toEqual(["signInAgain", "signOut"]);
  expect(cards[3]?.actions).toEqual(["connect"]);
  expect(cards[4]?.actions).toEqual(["signInHere"]);
  // A build without a Trakt app offers nothing to press.
  expect(cards[5]?.actions).toEqual([]);
});

test("each sign-in phase has its own words, and only a sign-in that ended offers a new code", () => {
  const phases: TraktLoginSnapshot[] = [
    { phase: "starting" },
    { phase: "code", userCode: "ABCD1234", url: "https://trakt.tv/activate", qr: "https://trakt.tv/activate", expiresMs: 0 },
    { phase: "done", user: "ana" },
    { phase: "expired" },
    { phase: "denied" },
    { phase: "notConfigured" },
    { phase: "failed", reason: "Trakt did not answer" },
  ];
  expect(new Set(phases.map(loginLine)).size).toBe(phases.length);
  expect(loginLine({ phase: "failed", reason: "Trakt did not answer" })).toBe("The sign-in did not finish: Trakt did not answer.");
  expect(phases.filter(loginRetry).map((p) => p.phase)).toEqual(["expired", "denied", "failed"]);
});

test("the code's time left counts down and never goes below zero", () => {
  expect(timeLeft(10 * 60_000, 0)).toBe("10:00");
  expect(timeLeft(65_500, 0)).toBe("1:06");
  expect(timeLeft(0, 5000)).toBe("0:00");
});

test("the player's mark: tracked, not tracked and why, and nothing for a profile without Trakt", () => {
  expect(playbackMark("on")).toEqual({ tone: "on", label: traktWords.playerOn });
  expect(playbackMark("unmapped")?.tone).toBe("warn");
  expect(playbackMark("thisDeviceOff")?.label).toBe(traktWords.playerThisDeviceOff);
  expect(playbackMark("off")).toBeNull();
  expect(playbackMark(undefined)).toBeNull();
});

test("waiting writes in words", () => {
  expect(pendingLine(0)).toBeNull();
  expect(pendingLine(1)).toBe("1 change waiting to go to Trakt");
  expect(pendingLine(3)).toBe("3 changes waiting to go to Trakt");
  expect(fill("{a} and {b}", { a: 1 })).toBe("1 and {b}");
});

test("Trakt copy uses no protocol words, dashes or emoji", () => {
  const dashes = String.fromCharCode(0x2013, 0x2014);
  const banned = new RegExp(`\\b(token|oauth|scrobble|api|uuid)s?\\b|[${dashes}]|--|\\p{Extended_Pictographic}`, "iu");
  for (const line of Object.values(traktWords)) expect(line).not.toMatch(banned);
});

test("every shared Trakt word is the web's word as it is", () => {
  for (const [key, line] of Object.entries(shared)) expect(traktWords[key as keyof typeof traktWords]).toBe(line);
});

const record = (generation: number, revision: number, account = "u1"): TraktRecord => ({
  accessToken: `a${generation}.${revision}`,
  refreshToken: "r",
  createdAtMs: 0,
  expiresAtMs: 1,
  scope: "public",
  account,
  user: "ana",
  revision,
  generation,
});

/** Records against a fake core and storage that log what they were asked, in order. */
function harness(persistFails = false) {
  const log: string[] = [];
  const stored: Record<string, TraktRecord>[] = [];
  const failing = { now: persistFails };
  const records = new TraktRecords({
    send: async (r) => void log.push(`send ${r.accessToken}`),
    persist: async (all) => {
      log.push(`persist ${Object.keys(all).sort().join(",") || "none"}`);
      if (failing.now) throw new Error("storage failed");
      stored.push(all);
    },
    forget: async (profile) => {
      log.push(`forget ${profile}`);
      if (failing.now) throw new Error("storage failed");
    },
  });
  return { log, stored, records, failing };
}
const open = () => true;

test("an offered record is stored first, then acknowledged", async () => {
  const { log, stored, records } = harness();
  await records.offered("p1", record(1, 0), open);
  expect(log).toEqual(["persist p1", "send a1.0"]);
  expect(stored[0]?.p1?.accessToken).toBe("a1.0");
  expect(records.get("p1")?.accessToken).toBe("a1.0");
});

test("a store that fails acknowledges nothing; Save again stores it", async () => {
  const { log, stored, records, failing } = harness(true);
  expect(await records.offered("p1", record(1, 0), open).then(() => "acknowledged", () => "refused")).toBe("refused");
  expect(log).toEqual(["persist p1"]);
  // Memory still holds it for this tab.
  expect(records.get("p1")).toBeDefined();
  failing.now = false;
  await records.save();
  expect(stored.at(-1)?.p1?.accessToken).toBe("a1.0");
});

test("an acknowledgement whose session ended (a switch or a lock meanwhile) is dropped", async () => {
  const { log, records } = harness();
  await records.offered("p1", record(1, 0), () => false);
  expect(log).toEqual(["persist p1"]);
  await records.resend("p1", () => false);
  expect(log).toEqual(["persist p1"]);
});

test("before the view opens the profile's record goes again; a profile with none sends nothing", async () => {
  const { log, records } = harness();
  await records.offered("p1", record(1, 0), open);
  log.length = 0;
  await records.resend("p1", open);
  await records.resend("p2", open);
  await records.resend("p1", open);
  expect(log).toEqual(["send a1.0", "send a1.0"]);
});

test("signing out deletes the profile's record, here and where it was sealed", async () => {
  const { log, stored, records } = harness();
  await records.offered("p1", record(1, 0), open);
  await records.offered("p2", record(1, 0, "u2"), open);
  log.length = 0;
  await records.signedOut("p1");
  expect(log).toEqual(["forget p1"]);
  expect(records.get("p2")).toEqual(record(1, 0, "u2"));
  await records.resend("p1", open);
  expect(log).toEqual(["forget p1"]);
});

test("a sealed copy that could not be deleted on sign-out is not reported as gone", async () => {
  const { records, failing } = harness();
  await records.offered("p1", record(1, 0), open);
  failing.now = true;
  expect(await records.signedOut("p1").then(() => "deleted", () => "kept")).toBe("kept");
});

test("records sealed here meet the ones in memory: the newer of each is kept", async () => {
  const { records } = harness();
  await records.offered("p1", record(2, 3), open);
  // The sealed copy is older for p1 and the only one for p2.
  expect(await records.restored({ p1: record(2, 1), p2: record(1, 0, "u2") }, "p1")).toBe(false);
  expect(records.get("p1")?.accessToken).toBe("a2.3");
  expect(records.get("p2")?.accessToken).toBe("a1.0");
  // A newer sealed copy replaces memory's, and says the open profile's changed.
  expect(await records.restored({ p1: record(3, 0) }, "p1")).toBe(true);
  expect(newer(record(3, 0), record(2, 9)).generation).toBe(3);
});

test("changes run in the order they came", async () => {
  const log: string[] = [];
  let release: () => void = () => undefined;
  const records = new TraktRecords({
    send: async (r) => void log.push(`send ${r.accessToken}`),
    persist: (all) =>
      new Promise((done) => {
        log.push(`persist ${Object.keys(all).join(",") || "none"}`);
        release = done;
      }),
    forget: (profile) =>
      new Promise((done) => {
        log.push(`forget ${profile}`);
        release = done;
      }),
  });
  const first = records.offered("p1", record(1, 0), open);
  const second = records.signedOut("p1");
  await new Promise((r) => setTimeout(r, 5));
  // The sign-out waits for the store and acknowledgement before it.
  expect(log).toEqual(["persist p1"]);
  release();
  await first;
  await new Promise((r) => setTimeout(r, 5));
  release();
  await second;
  expect(log).toEqual(["persist p1", "send a1.0", "forget p1"]);
});

test("Settings' Trakt line: an invitation when off, the state otherwise, and the link's words on a plain link", () => {
  expect(rowLine(null, true, clock)).toBe(traktWords.rowOff);
  expect(rowLine(status({ state: "off" }), true, clock)).toBe(traktWords.rowOff);
  expect(rowLine(status({ state: "connected" }), true, clock)).toBe("Connected as ana");
  expect(rowLine(status({ state: "connected" }), false, clock)).toBe(traktWords.plainLink);
});

test("a sign-in kept under a lock not opened offers Unlock, not Connect", () => {
  expect(stateCard(status({ state: "off" }), clock, true)).toEqual({ line: traktWords.lockedLine, actions: ["unlock"] });
  expect(stateCard(status({ state: "thisDeviceOff" }), clock, true).actions).toEqual(["unlock"]);
  // Connected meanwhile (the core holds one): as it is.
  expect(stateCard(status({ state: "connected" }), clock, true).actions).toEqual(["syncNow", "signOut"]);
  expect(rowLine(status({ state: "off" }), true, clock, null, true)).toBe(traktWords.lockedLine);
});

test("Settings' Trakt line says a storage problem first", () => {
  expect(rowLine(status({ state: "connected" }), true, clock, traktWords.saveFailed)).toBe(traktWords.saveFailed);
});
