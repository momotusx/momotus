import { expect, test } from "bun:test";
import type { MemberInfo } from "../core/generated/MemberInfo";
import { fingerprintGroups, isPairingCode, keepNewer, kitsGoing, lostDrop, memberName, sortMembers, sponsorName } from "./sync";
import { addedLine, joinProblem, lostButton, lostKits, removeDeviceAsk, reportNotes, syncProblem, syncWords } from "./words";

const member = (id: string, o: Partial<MemberInfo> = {}): MemberInfo => ({ id, kit: false, name: id.toUpperCase(), sponsor: null, addedMs: 0, thisDevice: false, ...o });
const quiet = { clockGapMs: 0, waiting: 0, parked: 0, oldEpochWrites: 0, rotationWaitMs: 0, blocked: null } as const;

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

test("the 12 characters read in groups of four", () => {
  expect(fingerprintGroups("VW4BV9DJD5EZ")).toBe("VW4B V9DJ D5EZ");
});

test("this device leads, then devices, then kits, oldest first", () => {
  const list = [member("k", { kit: true, addedMs: 1 }), member("b", { addedMs: 5 }), member("me", { thisDevice: true, addedMs: 9 }), member("a", { addedMs: 2 })];
  expect(sortMembers(list).map((m) => m.id)).toEqual(["me", "a", "b", "k"]);
  expect(memberName(member("k", { kit: true, name: null }))).toBe("Recovery kit");
  expect(memberName(member("x", { name: null }))).toBe("Unnamed device");
  expect(sponsorName(member("b", { sponsor: "a" }), list)).toBe("A");
  expect(sponsorName(member("b", { sponsor: "gone" }), list)).toBe("a removed device");
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
});

test("sync failures read by code, and never show the server's address", () => {
  expect(syncProblem("dropped", "this device was removed from the account")).toStartWith("This device was removed from your sync account.");
  expect(syncProblem("fork", "x")).toContain("Use the server's list");
  expect(syncProblem("unsent", "x")).toContain("stop anyway");
  expect(syncProblem(undefined, "roster: this device cannot report itself")).toBe("This device cannot report itself.");
  expect(syncProblem(undefined, "pairing: not a pairing code")).toBe("Not a pairing code.");
  for (const raw of ["network: error sending request for url (http://user:secret@relay.example/v1/push)", "http status 502", "timed out after 10s"]) {
    for (const words of [syncProblem(undefined, raw), joinProblem(undefined, raw, "start"), joinProblem(undefined, raw, "recover"), joinProblem(undefined, raw, "approve")]) {
      expect(words.includes("relay.example") || words.includes("secret") || words.includes("http")).toBe(false);
    }
  }
  expect(joinProblem(undefined, "http status 403", "start")).toContain("invite code");
  expect(joinProblem(undefined, "http status 404", "recover")).toBe(syncWords.wrongKit);
  expect(joinProblem(undefined, "http status 404", "approve")).toContain("expired or was used");
  expect(joinProblem(undefined, "pairing: the code is for another sync server", "approve")).toBe("The code is for another sync server.");
  expect(joinProblem("insecure", "the relay must use https unless it is on this network", "start")).toBe("The sync server address must start with https.");
});

test("device lines name the day and the device that added it", () => {
  const day = Date.UTC(2026, 9, 19, 12);
  expect(addedLine(false, day, null)).toBe("Added Oct 19, 2026, the first device");
  expect(addedLine(false, day, "Marquee phone")).toBe("Added Oct 19, 2026 through Marquee phone");
  expect(addedLine(true, day, "Marquee phone")).toBe("Made Oct 19, 2026 on Marquee phone");
  expect(lostButton(1)).toBe("Remove 1 device");
  expect(lostButton(3)).toBe("Remove 3 devices");
  expect(lostKits(1)).toStartWith("1 recovery kit made on these devices stops working.");
  expect(lostKits(2)).toStartWith("2 recovery kits made on these devices stop working.");
});

test("sync copy uses no protocol words, dashes or emoji", () => {
  const lines = [
    ...Object.values(syncWords),
    ...reportNotes({ clockGapMs: 9e6, waiting: 2, parked: 2, oldEpochWrites: 1, rotationWaitMs: 1, blocked: "relayDisagrees" }),
    ...["dropped", "fork", "wrongRelay", "relayBehind", "notJoined", "unsent", "insecure"].map((c) => syncProblem(c, "")),
    removeDeviceAsk("TV"),
    lostKits(2),
  ];
  // The long and short dash by code point, so this file holds neither.
  const dashes = String.fromCharCode(0x2013, 0x2014);
  const banned = new RegExp(`epoch|roster|fingerprint|keystore|relay|token|record|member|[${dashes}]|--|\\p{Extended_Pictographic}`, "iu");
  for (const line of lines) expect(line).not.toMatch(banned);
});
