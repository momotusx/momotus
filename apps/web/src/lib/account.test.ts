import { expect, test } from "bun:test";
import shared from "../../../shared/account-words.json";
import type { Announcement } from "../core/generated/Announcement";
import type { MemberInfo } from "../core/generated/MemberInfo";
import { aloneInAccount, announcementLine, continueWith, fill, homeCard, leaveChoice, providerAccount, providerName, providerOrder, revokeFailure, sameRelay, shown, signInFailure } from "./account";

const note = (kind: Announcement["kind"], more: Partial<Announcement> = {}): Announcement => ({
  id: `1.${kind}`,
  kind,
  member: "m1",
  kit: false,
  passkey: false,
  by: "d1",
  name: "Laptop",
  count: null,
  ...more,
});
const member = (id: string, name: string | null, kit = false) => ({ id, name, kit }) as MemberInfo;
const owner = { isOwner: true, kids: false };
const other = { isOwner: false, kids: false };
const kid = { isOwner: false, kids: true };

test("the shared file is read as it is, and its about names only keys it has", () => {
  const keys = new Set(Object.keys(shared));
  const named = shared.about.match(/\b(announce[A-Z]\w*|removeLibrary\w*|accountGoneActions|providerAccount\w*|providerName|passkeyLabel|debridConnected)\b/g) ?? [];
  expect(named.length).toBeGreaterThan(10);
  for (const key of named) expect(keys.has(key), key).toBe(true);
  // Every string is a sentence or a label, never empty, with no dash used as a pause.
  for (const [key, value] of Object.entries(shared)) {
    for (const text of typeof value === "string" ? [value] : Array.isArray(value) ? value : Object.values(value)) {
      expect(text.length, key).toBeGreaterThan(0);
      expect(text, key).not.toMatch(/\u2014| -- /);
    }
  }
});

/** Keys the web never shows, by why: it has no account passkey (D11) and approves no device (4.2). */
const NOT_ON_THE_WEB = {
  passkey: ["usePasskey", "passkeyHere", "passkeyLabel", "joinedWithPasskey", "lastUsed", "givingAway", "kitHere", "kitDropped", "tooManyKits", "kitTaken", "plainLink"],
  passkeySheet: ["noSecondWayIn", "noSecondWayInActions", "passkeyCancelled", "passkeyCancelledActions", "kitUnknownPasskey", "kitUnknownPasskeyActions"],
  approving: ["approveWarning", "scanAgainHere", "codeFound", "otherRelayCode", "otherRelayCodeActions"],
};

test("the web reads every key it shows, and only the passkey and approve keys stay unread", async () => {
  const { Glob } = await import("bun");
  let source = "";
  for await (const path of new Glob("src/**/*.{ts,tsx}").scan(".")) if (!path.endsWith(".test.ts")) source += await Bun.file(path).text();
  const skipped = new Set(Object.values(NOT_ON_THE_WEB).flat());
  for (const key of Object.keys(shared)) {
    if (key === "about") continue;
    expect(skipped.has(key) || new RegExp(`\\.${key}\\b|"${key}"`).test(source), `${key} is never read`).toBe(true);
  }
  // A key listed as unread exists, so the list cannot drift from the file.
  for (const key of skipped) expect(key in shared, key).toBe(true);
});

test("providers read as the apps word them, any other by its id", () => {
  expect(providerName("apple")).toBe("Apple");
  expect(providerName("mock")).toBe("mock");
  expect(providerAccount("google")).toBe("Google account");
  expect(providerAccount("mock")).toBe("mock sign-in");
  expect(continueWith("facebook")).toBe("Continue with Facebook");
  expect(providerOrder(["mock", "facebook", "apple", "google"])).toEqual(["apple", "google", "facebook", "mock"]);
});

test("each announcement kind takes its key, and every field is filled", () => {
  const members = [member("d1", "Phone"), member("m1", "Laptop")];
  const lines = [
    announcementLine(note("device"), members),
    announcementLine(note("device", { name: null }), members),
    announcementLine(note("passkey"), members),
    announcementLine(note("kit", { kit: true }), members),
    announcementLine(note("dropped", { member: null, by: null, count: 3 }), members),
    announcementLine(note("removed", { by: "m1" }), members),
    announcementLine(note("removed", { by: "m1", name: null }), members),
    announcementLine(note("removed", { kit: true, passkey: true }), members),
    announcementLine(note("removed", { kit: true }), members),
    announcementLine(note("removed"), members),
    announcementLine(note("removed", { name: null }), members),
  ];
  expect(lines).toEqual([
    "A new device joined: Laptop.",
    shared.announceDeviceUnnamed,
    "A passkey was added on Phone.",
    "A recovery kit was made on Phone.",
    shared.announceDropped,
    "Laptop signed out of your account.",
    shared.announceLeftUnnamed,
    "A passkey was removed from your account on Phone.",
    "A recovery kit was removed from your account on Phone.",
    "Laptop was removed from your account on Phone.",
    "A device was removed from your account on Phone.",
  ]);
  for (const line of lines) expect(line).not.toContain("{");
  // `{device}` falls back to a member with no name, then to one no longer in the account.
  expect(announcementLine(note("kit"), [member("d1", null)])).toBe(fill(shared.announceKit, { device: shared.announceUnnamedDevice }));
  expect(announcementLine(note("kit"), [])).toBe(fill(shared.announceKit, { device: shared.announceGoneDevice }));
});

test("only the owner clears a security notice; a kids profile shows none", () => {
  for (const kind of ["device", "passkey", "kit", "dropped"] as const) {
    expect(shown(note(kind), owner)).toEqual({ show: true, ok: true });
    expect(shown(note(kind), other)).toEqual({ show: true, ok: false });
    expect(shown(note(kind), kid)).toEqual({ show: false, ok: false });
  }
  expect(shown(note("removed"), other)).toEqual({ show: true, ok: true });
  expect(shown(note("removed"), kid)).toEqual({ show: true, ok: true });
});

test("Home's one account card goes by priority", () => {
  const date = () => "28 Oct";
  const base = { deletion: null, gone: false, announcements: [], members: null, profile: owner, alone: false, date };
  expect(homeCard(base)).toBeNull();
  expect(homeCard({ ...base, alone: true })?.kind).toBe("secondWay");
  // The nudge leads to a recovery kit, the owner's to make (D17).
  expect(homeCard({ ...base, alone: true, profile: other })).toBeNull();
  const withNote = { ...base, alone: true, announcements: [note("passkey")] };
  expect(homeCard(withNote)).toMatchObject({ kind: "announcement", ok: true, notYou: true });
  expect(homeCard({ ...withNote, profile: other })).toMatchObject({ kind: "announcement", ok: false, notYou: false });
  // A kids profile skips what it may not show, to the next one.
  expect(homeCard({ ...withNote, announcements: [note("passkey"), note("removed")], profile: kid })).toMatchObject({ kind: "announcement", id: "1.removed" });
  const gone = { ...withNote, gone: true };
  expect(homeCard(gone)).toEqual({ kind: "gone", text: shared.accountGone, remove: true });
  // Remove from this device is the owner's (D13).
  expect(homeCard({ ...gone, profile: other })).toMatchObject({ kind: "gone", remove: false });
  const asked = homeCard({ ...gone, deletion: { dueMs: 1, provider: "google" } });
  expect(asked).toMatchObject({ kind: "deletion", provider: "google" });
  expect(asked?.text).toBe("Someone signed in with your Google account and asked to delete your Marquee account on 28 Oct. Not you? Cancel, and turn off Google sign-in.");
  expect(homeCard({ ...gone, deletion: { dueMs: 1, provider: null } })?.text).toBe("Your account will be deleted on 28 Oct.");
});

test("a second way in is wanted while only this device holds the account", () => {
  expect(aloneInAccount(null)).toBe(false);
  expect(aloneInAccount([member("a", "Desk")])).toBe(true);
  expect(aloneInAccount([member("a", "Desk"), member("b", "Phone")])).toBe(false);
  expect(aloneInAccount([member("a", "Desk"), member("k", null, true)])).toBe(false);
});

test("every sign-in failure has its words and buttons, and a cancel says nothing", () => {
  const labels = (code: string) => signInFailure(code)?.actions.map((a) => a.label);
  const kinds = (code: string) => signInFailure(code)?.actions.map((a) => a.act);
  expect(signInFailure("cancelled")).toBeNull();
  // Section 5: each state's words and its buttons in order, the first the primary.
  expect(signInFailure("noRedirect")?.text).toBe(shared.noRedirect);
  expect(labels("noRedirect")).toEqual(shared.noRedirectActions);
  expect(kinds("noRedirect")).toEqual(["retry", "other"]);
  expect(signInFailure("ticketExpired")?.text).toBe("Sign in again to finish.");
  expect(kinds("ticketExpired")).toEqual(["retry"]);
  expect(signInFailure("signupLimit", "apple")?.text).toContain("This Apple Account has made several");
  expect(kinds("signupLimit")).toEqual(["other"]);
  expect(signInFailure("linkedElsewhere", "mock")?.text).toBe("This mock sign-in already signs in to another Marquee account.");
  expect(labels("linkedElsewhere")).toEqual(["Use it for this account instead", "Cancel"]);
  expect(kinds("linkedElsewhere")).toEqual(["move", "cancel"]);
  expect(labels("unreachable")).toEqual(["Try again", "Not now"]);
  expect(kinds("unreachable")).toEqual(["retry", "notNow"]);
  for (const code of ["busy", "otherRelay", "popupBlocked"]) expect(kinds(code)).toEqual(["retry"]);
  expect(signInFailure("notOwner")).toEqual({ text: shared.notOwner, actions: [] });
  expect(signInFailure("anything")?.text).toBe(shared.signInRefused);
});

test("Apple's revoke without a receipt goes on only when asked; its cancel says nothing", () => {
  expect(revokeFailure("cancelled", "Unlink anyway", "Cancel")).toBeNull();
  expect(revokeFailure("unreachable", shared.deleteAnyway, "Cancel")).toEqual({
    text: shared.appleRevokeFailed,
    actions: [
      { label: "Delete anyway", act: "anyway" },
      { label: "Cancel", act: "cancel" },
    ],
  });
});

test("sign out removes the library only on the owner profile, always on a TV (D13)", () => {
  expect(leaveChoice(owner, false)).toEqual({ show: true, ask: true, always: false });
  expect(leaveChoice(other, false)).toEqual({ show: true, ask: false, always: false });
  expect(leaveChoice(kid, false)).toEqual({ show: true, ask: false, always: false });
  expect(leaveChoice(owner, true)).toEqual({ show: true, ask: false, always: true });
  // A TV's Sign out always removes, so another profile there has none.
  expect(leaveChoice(other, true)).toEqual({ show: false, ask: false, always: false });
  expect(leaveChoice(null, true).show).toBe(false);
});

test("a return from another relay is never handed on", () => {
  expect(sameRelay("flow=f&code=c&relay=sync.example.org", "https://sync.example.org")).toBe(true);
  expect(sameRelay("flow=f&code=c&relay=127.0.0.1:17818", "http://127.0.0.1:17818")).toBe(true);
  expect(sameRelay("flow=f&code=c&relay=evil.example", "https://sync.example.org")).toBe(false);
});
