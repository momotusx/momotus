import { expect, test } from "bun:test";
import shared from "../../../shared/account-words.json";
import type { Announcement } from "../core/generated/Announcement";
import type { MemberInfo } from "../core/generated/MemberInfo";
import {
  announcementLine,
  cancelUnlink,
  codeDigits,
  connectGoogle,
  connectText,
  connectTitle,
  continueWith,
  continueWithEmail,
  emailEnds,
  emailFailure,
  emailFieldError,
  fill,
  homeCard,
  leaveChoice,
  needsSecondWayIn,
  providerAccount,
  providerName,
  providerOrder,
  resendIn,
  revokeFailure,
  sameRelay,
  shown,
  signInFailure,
} from "./account";

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
const member = (id: string, name: string | null, kit = false, form: string | null = null, thisDevice = false) => ({ id, name, kit, form, thisDevice }) as MemberInfo;
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
  passkey: ["usePasskey", "passkeyHere", "savePasskeyHere", "passkeyLabel", "joinedWithPasskey", "lastUsed", "givingAway", "kitHere", "kitDropped", "tooManyKits", "kitTaken", "plainLink"],
  passkeySheet: ["noSecondWayIn", "noSecondWayInActions", "passkeyCancelled", "passkeyCancelledActions", "kitUnknownPasskey", "kitUnknownPasskeyActions"],
  approving: ["approveWarning", "scanAgainHere", "codeFound", "otherRelayCode", "otherRelayCodeActions", "approveAsk", "approveAskTv", "approveAskOther", "approveButton", "approveNear", "approveUnnamed", "approveScanHere", "approveDone", "approveNotYou", "approveSlow", "approveAnswered", "approveExpired", "approveKids"],
  // The TV's screen before plans/tv-qr-login.md, still read by the native apps until their phases.
  retired: ["tvSignInHow", "tvWithoutAccount"],
  // The native first run's passkey probe: the web shows Create at once (D11).
  nativeFirstRun: ["continue"],
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
  const base = { deletion: null, gone: false, announcements: [], members: null, profile: owner, alone: false, signedInFrom: null, date };
  expect(homeCard(base)).toBeNull();
  expect(homeCard({ ...base, alone: true })?.kind).toBe("secondWay");
  // The nudge leads to a recovery kit, the owner's to make (D17).
  expect(homeCard({ ...base, alone: true, profile: other })).toBeNull();
  // The device that signed this one in comes before announcements, on every profile but a kids one.
  const fresh = { ...base, alone: true, signedInFrom: "Pixel", announcements: [note("passkey")] };
  expect(homeCard(fresh)).toEqual({ kind: "signedIn", text: "Signed in from Pixel" });
  expect(homeCard({ ...fresh, profile: other })?.kind).toBe("signedIn");
  expect(homeCard({ ...fresh, profile: kid })).toBeNull();
  const withNote = { ...base, alone: true, announcements: [note("passkey")] };
  expect(homeCard(withNote)).toMatchObject({ kind: "announcement", ok: true, notYou: true, member: "m1" });
  expect(homeCard({ ...withNote, profile: other })).toMatchObject({ kind: "announcement", ok: false, notYou: false });
  // A kids profile reaches no account page: no card at all, not even the deletion or the account gone.
  for (const state of [withNote, { ...withNote, announcements: [note("removed")] }, { ...withNote, gone: true }, { ...withNote, deletion: { dueMs: 1, provider: "google" } }]) expect(homeCard({ ...state, profile: kid })).toBeNull();
  const gone = { ...withNote, gone: true };
  expect(homeCard(gone)).toEqual({ kind: "gone", text: shared.accountGone, remove: true });
  // Remove from this device is the owner's (D13).
  expect(homeCard({ ...gone, profile: other })).toMatchObject({ kind: "gone", remove: false });
  const asked = homeCard({ ...gone, deletion: { dueMs: 1, provider: "google" } });
  expect(asked).toMatchObject({ kind: "deletion", provider: "google" });
  expect(asked?.text).toBe("Someone signed in with your Google account and asked to delete your Momotus account on 28 Oct. Not you? Cancel, and turn off Google sign-in.");
  expect(homeCard({ ...gone, deletion: { dueMs: 1, provider: null } })?.text).toBe("Your account will be deleted on 28 Oct.");
  // Email has its own words: it is an address, not an account (plans/accounts-email.md 4).
  expect(homeCard({ ...gone, deletion: { dueMs: 1, provider: "email" } })?.text).toBe(
    "Someone signed in with your email address and asked to delete your Momotus account on 28 Oct. Not you? Cancel, and turn off email sign-in.",
  );
  expect(cancelUnlink("email")).toBe("Cancel and turn off email sign-in");
  expect(cancelUnlink("google")).toBe("Cancel and turn off Google sign-in");
  // A sign-up without a sign-in that hit its network's limit says so, with no provider named.
  expect(signInFailure("signupLimit")?.text).toBe(shared.signupLimitNetwork);
  expect(signInFailure("signupLimit", "google")?.text).toContain("Google account");
});

test("a second way in is a recovery kit or another phone or tablet, as the protocol says", () => {
  const desk = member("a", "Desk", false, "web", true);
  expect(needsSecondWayIn(null)).toBe(false);
  expect(needsSecondWayIn([desk])).toBe(true);
  expect(needsSecondWayIn([desk, member("k", null, true)])).toBe(false);
  expect(needsSecondWayIn([desk, member("b", "Phone", false, "phone")])).toBe(false);
  expect(needsSecondWayIn([desk, member("b", "Tablet", false, "tablet")])).toBe(false);
  // A TV, another browser or a computer signs nobody in: still wanted.
  expect(needsSecondWayIn([desk, member("t", "TV", false, "tv"), member("w", "Laptop", false, "web"), member("c", "Mac", false, "computer")])).toBe(true);
  // This device itself, a phone, is not its own second way in.
  expect(needsSecondWayIn([member("p", "Phone", false, "phone", true)])).toBe(true);
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
  expect(signInFailure("linkedElsewhere", "mock")?.text).toBe("This mock sign-in already signs in to another Momotus account.");
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
  // A relay is named by its host alone: its port may change when it moves (mq-sync host()).
  expect(sameRelay("flow=f&code=c&relay=127.0.0.1", "http://127.0.0.1:17818")).toBe(true);
  expect(sameRelay("flow=f&code=c&relay=evil.example", "https://sync.example.org")).toBe(false);
});

test("email is never a provider button: its own row, named Email in links", () => {
  expect(providerOrder(["email", "mock", "google", "apple"])).toEqual(["apple", "google", "mock"]);
  expect(providerOrder(["email"])).toEqual([]);
  expect(continueWithEmail).toBe("Continue with email");
  expect(providerName("email")).toBe("Email");
  expect(signInFailure("signupLimit", "email")?.text).toBe("This email address has made several Momotus accounts lately. Try again later or use another way.");
});

test("each email answer takes the answers table's words and buttons", () => {
  const labels = (code: string) => emailFailure(code).actions.map((a) => `${a.label}:${a.act}`);
  expect(emailFailure("ticketExpired").text).toBe(shared.codeExpired);
  expect(labels("wrongFlow")).toEqual(["Send a new code:resend", "Use another address:otherAddress"]);
  // signInRefused keeps its own words, with the code stage's way on.
  expect(emailFailure("signInRefused").text).toBe(shared.signInRefused);
  expect(labels("signInRefused")).toEqual(labels("ticketExpired"));
  // emailWait has no button: Send code stays usable.
  expect(emailFailure("emailWait")).toEqual({ text: shared.emailWait, actions: [] });
  expect(labels("emailLimit")).toEqual(["Other ways to sign in:other"]);
  expect(labels("mailUnavailable")).toEqual(["Try again:retry", "Other ways to sign in:other"]);
  expect(emailFailure("busy")).toEqual(signInFailure("busy") as never);
  // No coded answer: the relay did not answer, and Try again repeats the stage's call.
  expect(emailFailure("")).toEqual(signInFailure("unreachable") as never);
  expect(labels("")[0]).toBe("Try again:retry");
  // Lines under the field, not failures.
  expect(emailFieldError("badAddress")).toBe("Enter an email address like name@example.com.");
  expect(emailFieldError("wrongCode")).toBe(shared.wrongCode);
  expect(emailFieldError("badCode")).toBe(shared.wrongCode);
  expect(emailFieldError("busy")).toBeNull();
  // These end the code's flow; a wrong code or no answer keeps it.
  for (const code of ["ticketExpired", "wrongFlow", "signInRefused", "emailLimit"]) expect(emailEnds(code), code).toBe(true);
  for (const code of ["wrongCode", "badCode", "busy", "", "emailWait"]) expect(emailEnds(code), code).toBe(false);
});

test("the code field keeps digits only and cuts at 6; Send a new code waits 60 s from the mail", () => {
  expect(codeDigits("123 456")).toBe("123456");
  expect(codeDigits("123-456")).toBe("123456");
  expect(codeDigits("12a3")).toBe("123");
  expect(codeDigits("1234567")).toBe("123456");
  expect(resendIn(1000, 1000)).toBe(60);
  expect(resendIn(1000, 1001)).toBe(60);
  expect(resendIn(1000, 59_500)).toBe(2);
  expect(resendIn(1000, 61_000)).toBe(0);
  // Time in the background counts: the count comes from the mail's time, not from ticks.
  expect(resendIn(1000, 3_600_000)).toBe(0);
});

test("Settings offers Connect Google only when the relay has Google and the read links lack it", () => {
  expect(connectGoogle(["google", "email"], ["email"])).toBe(true);
  expect(connectGoogle(["google"], [])).toBe(true);
  expect(connectGoogle(["google"], ["google"])).toBe(false);
  expect(connectGoogle(["facebook"], [])).toBe(false);
  // Links not read yet: no row that might name a link the account already has.
  expect(connectGoogle(["google"], undefined)).toBe(false);
  expect(connectTitle("google")).toBe("Connect Google");
  expect(connectText("google")).toBe("Sign in with your Google account too.");
});
