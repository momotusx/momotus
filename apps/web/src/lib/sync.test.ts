import { expect, test } from "bun:test";
import type { MemberInfo } from "../core/generated/MemberInfo";
import {
  fingerprintGroups,
  forkDrop,
  forkKits,
  forkSides,
  isPairingCode,
  isPrivateLink,
  keepNewer,
  kitsGoing,
  lostDrop,
  lostRows,
  memberName,
  sortMembers,
  sponsorName,
  watchForCode,
} from "./sync";
import {
  addedLine,
  failedSince,
  forkButton,
  goesWith,
  lostButton,
  lostKits,
  outcomeLine,
  outcomeOf,
  removeDeviceAsk,
  reportNotes,
  stepProblem,
  syncProblem,
  syncSummary,
  syncWords,
} from "./words";

const member = (id: string, o: Partial<MemberInfo> = {}): MemberInfo => ({ id, kit: false, name: id.toUpperCase(), sponsor: null, addedMs: 0, thisDevice: false, ...o });
const quiet = { clockGapMs: 0, waiting: 0, parked: 0, oldEpochWrites: 0, rotationWaitMs: 0, blocked: null, refused: 0, unreadable: 0, newer: 0, again: false } as const;

test("the record with the highest revision is kept", () => {
  const held = { revision: 3, token: "a" };
  expect(keepNewer(held, { revision: 2, token: "b" })).toBe(held);
  const next = { revision: 4, token: "c" };
  expect(keepNewer(held, next)).toBe(next);
  const same = { revision: 3, token: "d" };
  expect(keepNewer(held, same)).toBe(same);
  expect(keepNewer(null, same)).toBe(same);
});

test("only a pairing code is taken from the camera", () => {
  expect(isPairingCode(" marquee:pair?v=2&s=x&p=y ")).toBe(true);
  expect(isPairingCode("https://example.org/marquee:pair?")).toBe(false);
  expect(isPairingCode("")).toBe(false);
});

test("a look that ends after Cancel approves nothing", async () => {
  let stopped = false;
  const found: string[] = [];
  // The camera sees a code, but the user cancels while the reader works.
  const detect = async () => {
    stopped = true;
    return ["marquee:pair?v=2&s=https%3A%2F%2Fsync.example.org&p=1"];
  };
  await watchForCode(detect, () => stopped, (c) => found.push(c), async () => undefined);
  expect(found).toEqual([]);
  // Without the cancel the same look approves, and other codes are skipped.
  let looks = 0;
  const seen = [["https://example.org"], [" marquee:pair?v=2&p=2 "]];
  await watchForCode(
    async () => seen[looks++] ?? [],
    () => false,
    (c) => found.push(c),
    async () => undefined,
  );
  expect(found).toEqual(["marquee:pair?v=2&p=2"]);
});

test("the question before approving a scanned code names no host", () => {
  // The host is always this account's own server: naming it would read as a check it is not.
  expect(syncWords.reviewAsk).toBe("Approve a new device?");
});

test("the record goes only over an encrypted link or one on this machine", () => {
  for (const ok of ["ws://127.0.0.1:7777/ws", "ws://localhost:7777/ws", "ws://[::1]:7777/ws", "wss://tv.local/ws"]) expect(isPrivateLink(ok)).toBe(true);
  for (const ok of ["ws://127.1.2.3:7777/ws", "ws://127.255.255.255/ws"]) expect(isPrivateLink(ok)).toBe(true);
  // A name that only starts like a loopback address is someone else's host.
  for (const bad of ["ws://192.168.1.20:7777/ws", "ws://tv.local:7777/ws", "ws://127.0.0.1.evil.com/ws", "ws://127.evil.com/ws", "ws://127-0-0-1.example/ws", "not a url"])
    expect(isPrivateLink(bad)).toBe(false);
});

test("the 12 characters read in groups of four", () => {
  expect(fingerprintGroups("VW4BV9DJD5EZ")).toBe("VW4B V9DJ D5EZ");
});

test("this device leads, then devices, then kits, oldest first", () => {
  const list = [member("k", { kit: true, addedMs: 1 }), member("b", { addedMs: 5 }), member("me", { thisDevice: true, addedMs: 9 }), member("a", { addedMs: 2 })];
  expect(sortMembers(list).map((m) => m.id)).toEqual(["me", "a", "b", "k"]);
  expect(memberName(member("k", { kit: true, name: null }))).toBe("Recovery kit");
  expect(memberName(member("x", { name: null }))).toBe("Unnamed device");
  expect(sponsorName(member("b", { sponsor: "a" }), list)).toBe("A");
  expect(sponsorName(member("b", { sponsor: "gone" }), list)).toBe("a device no longer in your account");
  expect(sponsorName(member("a"), list)).toBeNull();
});

test("a lost report drops what the user left ticked, and the kits of what goes", () => {
  // B is lost; B made kit K; C joined through K; C made kit K2; A made kit K3.
  const members = [
    member("a", { thisDevice: true }),
    member("b", { sponsor: "a" }),
    member("k", { kit: true, sponsor: "b" }),
    member("c", { sponsor: "k" }),
    member("k2", { kit: true, sponsor: "c" }),
    member("k3", { kit: true, sponsor: "a" }),
  ];
  const preview = { devices: ["c"], kits: ["k", "k2"] };
  expect(lostDrop("b", preview.devices, new Set())).toEqual(["b", "c"]);
  expect(kitsGoing(preview.kits, members, lostDrop("b", preview.devices, new Set()))).toEqual(["k", "k2"]);
  // Unticking C keeps C and the kit C made; the kit B made still goes.
  const drop = lostDrop("b", preview.devices, new Set(["c"]));
  expect(drop).toEqual(["b"]);
  expect(kitsGoing(preview.kits, members, drop)).toEqual(["k"]);
  // A kit whose maker is already gone always goes.
  expect(kitsGoing(["k4"], [...members, member("k4", { kit: true, sponsor: "gone" })], ["b"])).toEqual(["k4"]);
});

test("every device a lost report drops has a row, also one the list does not know yet", () => {
  const rows = lostRows(["c", "tablet"], [member("c")]);
  expect(rows.map((r) => [r.id, r.member?.name ?? null])).toEqual([
    ["c", "C"],
    ["tablet", null],
  ]);
});

test("both sides of a different list, and what continuing removes", () => {
  const mine = [member("me", { thisDevice: true }), member("tv"), member("kit1", { kit: true })];
  const server = [member("me", { thisDevice: true }), member("tv"), member("laptop"), member("kit2", { kit: true })];
  const sides = forkSides(mine, server);
  expect(sides.both.map((m) => m.id)).toEqual(["me", "tv"]);
  expect(sides.serverOnly.map((m) => m.id)).toEqual(["laptop", "kit2"]);
  expect(sides.mineOnly.map((m) => m.id)).toEqual(["kit1"]);
  // The server's own drops go anyway; ticks add to them; this device never.
  expect(forkDrop(server, ["kit2"], new Set(["laptop", "me", "kit2"]))).toEqual(["laptop"]);
  expect(forkDrop(server, ["kit2"], new Set())).toEqual([]);
  expect(forkButton(0)).toBe("Continue with the server's list");
  expect(forkButton(2)).toBe("Continue and remove 2 devices");
  // A ticked device takes the kits it made along, never the server's own drops twice.
  const kits = [
    { device: "laptop", kits: ["kl"] },
    { device: "tv", kits: ["kt", "kit2"] },
  ];
  expect(forkKits(kits, ["laptop"], ["kit2"])).toEqual(["kl"]);
  expect(forkKits(kits, ["laptop", "tv"], ["kit2"])).toEqual(["kl", "kt"]);
  expect(forkKits(kits, [], [])).toEqual([]);
  expect(forkButton(1, 1)).toBe("Continue and remove 1 device and 1 recovery kit");
  expect(forkButton(2, 3)).toBe("Continue and remove 2 devices and 3 recovery kits");
  // A ticked kit counts as a kit, never as a device.
  expect(forkButton(0, 2)).toBe("Continue and remove 2 recovery kits");
  expect(goesWith(1)).toBe("Its recovery kit goes with it.");
  expect(goesWith(2)).toBe("Its 2 recovery kits go with it.");
});

test("a round is complete, waiting or failed, never green while changes failed", () => {
  expect(outcomeOf(quiet)).toBe("complete");
  for (const pending of [{ waiting: 1 }, { parked: 1 }, { newer: 1 }, { blocked: "notActive" as const }, { rotationWaitMs: 5 }, { again: true }])
    expect(outcomeOf({ ...quiet, ...pending })).toBe("pending");
  expect(outcomeOf({ ...quiet, refused: 1, waiting: 3 })).toBe("failed");
  expect(outcomeOf({ ...quiet, unreadable: 2 })).toBe("failed");
  expect(outcomeLine("complete", "2:05 PM")).toBe("Synced at 2:05 PM");
  expect(outcomeLine("pending", "2:05 PM")).toBe("Synced at 2:05 PM. Some changes still wait.");
  expect(outcomeLine("failed", "2:05 PM")).toBe("Synced at 2:05 PM. Some changes did not sync.");
  expect(failedSince(0, 0)).toBeNull();
  expect(failedSince(1, 0)).toBe("Since this device joined, 1 edit from this device could not be sent and stays here only.");
  expect(failedSince(2, 3)).toBe("Since this device joined, 2 edits from this device could not be sent and stay here only, and 3 changes from other devices could not be read.");
  expect(failedSince(0, 0, 1)).toBe("Since this device joined, 1 change from a newer version of Marquee waits for an update here.");
});

test("each fact of a sync round has its sentence, and a quiet round has none", () => {
  expect(reportNotes(quiet)).toEqual([]);
  expect(reportNotes({ ...quiet, clockGapMs: 600_000 })).toEqual([]);
  expect(reportNotes({ ...quiet, clockGapMs: -900_000 })[0]).toBe("The sync server's clock and this device's disagree by 15 minutes, so edits sync in the order they are sent.");
  expect(reportNotes({ ...quiet, waiting: 1 })[0]).toBe("1 edit waits for the sync server's clock to catch up. Nothing is lost.");
  expect(reportNotes({ ...quiet, waiting: 3 })[0]).toStartWith("3 edits wait");
  expect(reportNotes({ ...quiet, parked: 2 })[0]).toBe("2 changes from other devices wait until this device catches up with a device change.");
  expect(reportNotes({ ...quiet, oldEpochWrites: 1 })[0]).toContain("Compare the 12 characters below on each device.");
  expect(reportNotes({ ...quiet, rotationWaitMs: 61_000 })[0]).toStartWith("Edits wait 2 minutes more");
  expect(reportNotes({ ...quiet, rotationWaitMs: 1 })[0]).toStartWith("Edits wait 1 minute more");
  expect(reportNotes({ ...quiet, blocked: "rotationPending" })).toEqual(["Edits wait until the account finishes a device change."]);
  expect(reportNotes({ ...quiet, blocked: "notActive" })[0]).toContain("does not take edits from this device");
  expect(reportNotes({ ...quiet, blocked: "relayDisagrees" })[0]).toContain("disagrees about your device list");
  expect(reportNotes({ ...quiet, refused: 1 })[0]).toBe("1 edit from this device could not be sent: the sync server does not take it, so it stays on this device only.");
  expect(reportNotes({ ...quiet, refused: 2 })[0]).toContain("does not take them, so they stay");
  expect(reportNotes({ ...quiet, unreadable: 1 })[0]).toBe("1 change from another device could not be read and was skipped.");
  expect(reportNotes({ ...quiet, newer: 3 })[0]).toBe("3 changes come from a newer version of Marquee. Update Marquee here to see them.");
  expect(reportNotes({ ...quiet, again: true })[0]).toContain("stopped before it finished");
});

test("sync failures read by code and by action, and never show the server's address", () => {
  expect(syncProblem("dropped", "this device was removed from the account")).toStartWith("This device was removed from your sync account.");
  expect(syncProblem("fork", "x")).toContain("Compare the lists");
  expect(syncProblem("unsent", "x")).toContain("stop anyway");
  expect(syncProblem("passkey", "x")).toBe("The passkey did not confirm it, so nothing changed.");
  // A passkey without PRF is not a server that did not answer.
  expect(syncProblem("noPrf", "this passkey has no PRF")).toBe(syncWords.noPrf);
  expect(syncProblem("rosterChanged", "x")).toBe(syncWords.forkChanged);
  // Remembering fails in this browser's storage, never on the sync server.
  expect(stepProblem(undefined, "AbortError", "remember")).toBe(syncWords.storageFailed);
  expect(stepProblem("passkey", "x", "remember")).toBe("The passkey did not confirm it, so nothing changed.");
  expect(syncProblem("forget", "x")).toBe(syncWords.forgetFailed);
  expect(stepProblem(undefined, "http status 502", "differ")).toStartWith("The approval is still open.");
  // A relay that lost this device's token cannot be read: it says so, never "try again".
  expect(stepProblem(undefined, "http status 401", "compare")).toBe(syncWords.forkTokenLost);
  expect(stepProblem(undefined, "http status 502", "compare")).toStartWith("The lists could not be compared.");
  expect(stepProblem("noPrf", "x", "unlock")).toBe(syncWords.noPrf);
  expect(syncProblem("plainLink", "x")).toBe(syncWords.plainLink);
  expect(syncProblem("alreadyJoined", "x")).toBe(syncWords.alreadyJoined);
  expect(syncProblem(undefined, "roster: this device cannot report itself")).toBe("This device cannot report itself.");
  expect(syncProblem(undefined, "pairing: not a pairing code")).toBe("Not a pairing code.");
  const steps = ["start", "recover", "approve", "load", "kit", "remove", "lost", "leave", "serverList", "unlock", "compare", "differ", "remember"] as const;
  for (const raw of ["network: error sending request for url (http://user:secret@relay.example/v1/push)", "http status 502", "timed out after 10s"]) {
    for (const words of [syncProblem(undefined, raw), ...steps.map((s) => stepProblem(undefined, raw, s))]) {
      expect(words.includes("relay.example") || words.includes("secret") || words.includes("http")).toBe(false);
    }
  }
  expect(stepProblem(undefined, "http status 403", "start")).toContain("invite code");
  expect(stepProblem(undefined, "http status 404", "recover")).toBe(syncWords.wrongKit);
  expect(stepProblem(undefined, "http status 404", "approve")).toContain("expired or was used");
  // A 404 elsewhere is no expired code: each action says what did not happen.
  expect(stepProblem(undefined, "http status 404", "kit")).toBe("No kit was made. The sync server did not answer. Try again in a moment.");
  expect(stepProblem(undefined, "http status 502", "remove")).toStartWith("Nothing was removed.");
  expect(stepProblem(undefined, "http status 502", "load")).toStartWith("The device list did not load.");
  expect(stepProblem(undefined, "http status 502", "leave")).toStartWith("This device still syncs.");
  expect(stepProblem(undefined, "pairing: the code is for another sync server", "approve")).toBe("The code is for another sync server.");
  expect(stepProblem("insecure", "the relay must use https unless it is on this network", "start")).toBe("The sync server address must start with https.");
});

test("device lines name the day, the time and the device that added it", () => {
  const day = Date.UTC(2026, 9, 19, 12);
  const time = String.raw`\d{1,2}:\d{2}\s[AP]M`;
  expect(addedLine(false, day, null)).toMatch(new RegExp(`^Added Oct 19, 2026, ${time}, the first device$`));
  expect(addedLine(false, day, "Marquee phone")).toMatch(new RegExp(`^Added Oct 19, 2026, ${time} through Marquee phone$`));
  expect(addedLine(true, day, "Marquee phone")).toMatch(new RegExp(`^Made Oct 19, 2026, ${time} on Marquee phone$`));
  expect(lostButton(1)).toBe("Remove 1 device");
  expect(lostButton(3)).toBe("Remove 3 devices");
  expect(lostKits(1)).toStartWith("1 recovery kit made on these devices stops working.");
  expect(lostKits(2)).toStartWith("2 recovery kits made on these devices stop working.");
});

test("sync copy uses no protocol words, dashes or emoji", () => {
  const lines = [
    ...Object.values(syncWords),
    ...reportNotes({ clockGapMs: 9e6, waiting: 2, parked: 2, oldEpochWrites: 1, rotationWaitMs: 1, blocked: "relayDisagrees", refused: 1, unreadable: 1, newer: 1, again: true }),
    ...["dropped", "fork", "wrongRelay", "relayBehind", "notJoined", "unsent", "insecure", "passkey"].map((c) => syncProblem(c, "")),
    removeDeviceAsk("TV"),
    lostKits(2),
    failedSince(2, 2) ?? "",
  ];
  // The long and short dash by code point, so this file holds neither.
  const dashes = String.fromCharCode(0x2013, 0x2014);
  const banned = new RegExp(`\\b(epoch|roster|fingerprint|keystore|relay|token|record|member)s?\\b|[${dashes}]|--|\\p{Extended_Pictographic}`, "iu");
  for (const line of lines) expect(line).not.toMatch(banned);
});

test("Settings names what failed or waits before the time, and a lock or a problem before that", () => {
  const base = { locked: false, problem: null, joined: true, refused: 0, unreadable: 0, report: quiet, time: "2:05 PM" };
  expect(syncSummary(base)).toBe("On. Synced at 2:05 PM.");
  expect(syncSummary({ ...base, report: { ...quiet, blocked: "notActive" } })).toBe("The sync server does not take edits from this device yet, so they were not sent.");
  expect(syncSummary({ ...base, report: { ...quiet, refused: 1 } })).toStartWith("1 edit from this device could not be sent");
  expect(syncSummary({ ...base, refused: 2 })).toStartWith("Since this device joined, 2 edits");
  expect(syncSummary({ ...base, newer: 2 })).toContain("2 changes from a newer version of Marquee wait");
  expect(syncSummary({ ...base, problem: "The sync server did not answer." })).toBe("The sync server did not answer.");
  expect(syncSummary({ ...base, joined: false })).toBe(syncWords.offLine);
  expect(syncSummary({ ...base, joined: false, locked: true })).toBe(syncWords.locked);
});
