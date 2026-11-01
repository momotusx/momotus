/** Accounts on the web (plans/accounts.md 4.1 to 4.3, 5): the words of every
 * sign-in state and card, and the rules that pick them, kept apart from the
 * screens so both can be tested. Every word comes from
 * `apps/shared/account-words.json`, the file the apps read too; its `about`
 * holds the rules this file follows. */

import shared from "../../../shared/account-words.json";
import type { Announcement } from "../core/generated/Announcement";
import type { MemberInfo } from "../core/generated/MemberInfo";
import type { ProfilesSnapshot } from "../core/generated/ProfilesSnapshot";
import { current } from "./profiles";
import type { ScheduledDeletion } from "../core/generated/ScheduledDeletion";

export const accountWords = shared;
const w = shared;

/** `{name}` fields filled; a field with no value stays as written. */
export const fill = (text: string, fields: Record<string, string | number>) => text.replace(/\{(\w+)\}/g, (all, key: string) => (key in fields ? String(fields[key]) : all));

type Named = keyof typeof w.providerName;
/** A provider as people read it: Apple, Google, Facebook, or another OpenID provider's id as it is. */
export const providerName = (id: string) => (id in w.providerName ? w.providerName[id as Named] : id);
/** "Apple Account", "Google account", ..., else "<provider> sign-in". */
export const providerAccount = (id: string) => (id in w.providerAccount ? w.providerAccount[id as Named] : fill(w.providerAccountOther, { provider: providerName(id) }));
export const continueWith = (id: string) => fill(w.continueWith, { provider: providerName(id) });
/** Settings' Connect Google: the relay offers Google and the account's links, once read, have none. */
export const connectGoogle = (offered: readonly string[], linked: readonly string[] | undefined) => !!linked && offered.includes("google") && !linked.includes("google");
export const connectTitle = (id: string) => fill(w.connectProvider, { provider: providerName(id) });
export const connectText = (id: string) => fill(w.connectProviderText, { providerAccount: providerAccount(id) });

/** The order of a relay's providers on the first screen: Apple, Google and Facebook first, as
 * the web shows no platform's own sheet (4.1.1), then any other in the relay's order. */
export const providerOrder = (ids: readonly string[]) => {
  // Email is never a provider button: it is its own "Continue with email" row (plans/accounts-email.md 4).
  const buttons = ids.filter((id) => id !== EMAIL);
  const known = Object.keys(w.providerName);
  return [...buttons.filter((id) => id in w.providerName).sort((a, b) => known.indexOf(a) - known.indexOf(b)), ...buttons.filter((id) => !(id in w.providerName))];
};

export const EMAIL = "email";
export const continueWithEmail = w.continueWithEmail;

/** What a button under a failure does: sign in again with the same provider, the other ways,
 * Not now, move the sign-in to this account, go on without Apple's receipt, or put the line away;
 * on the email code stage, Send a new code and Use another address. */
export type Act = "retry" | "other" | "notNow" | "move" | "anyway" | "cancel" | "resend" | "otherAddress";
export interface Action {
  label: string;
  act: Act;
}

/** What a sign-in that did not end in an account says, and the buttons under it (first is the
 * primary); null when it says nothing (a cancelled provider page: back on the sign-in screen). */
export interface Failure {
  text: string;
  actions: Action[];
}

/** The words' labels in order, each with what it does. */
const acts = (labels: readonly string[], kinds: Act[]): Action[] => labels.map((label, i) => ({ label, act: kinds[i] ?? "cancel" }));
const retry = () => acts(w.noRedirectActions.slice(0, 1), ["retry"]);

export function signInFailure(code: string, provider = ""): Failure | null {
  const account = providerAccount(provider);
  switch (code) {
    case "cancelled":
      return null;
    case "noRedirect":
      return { text: w.noRedirect, actions: acts(w.noRedirectActions, ["retry", "other"]) };
    case "ticketExpired":
      return { text: w.ticketExpired, actions: acts(w.ticketExpiredActions, ["retry"]) };
    case "signupLimit":
      // Without a provider: the sign-up without a sign-in, counted per network (plans/tv-qr-login.md Q7).
      return { text: provider ? fill(w.signupLimit, { providerAccount: account }) : w.signupLimitNetwork, actions: acts(w.signupLimitActions, ["other"]) };
    case "linkedElsewhere":
      return { text: fill(w.linkedElsewhere, { providerAccount: account }), actions: acts(w.linkedElsewhereActions, ["move", "cancel"]) };
    case "busy":
      return { text: w.busy, actions: retry() };
    case "otherRelay":
      return { text: w.otherRelay, actions: retry() };
    case "popupBlocked":
      return { text: w.popupBlocked, actions: retry() };
    case "notOwner":
      return { text: w.notOwner, actions: [] };
    case "unreachable":
      return { text: w.unreachable, actions: acts(w.unreachableActions, ["retry", "notNow"]) };
    default:
      return { text: w.signInRefused, actions: acts(w.noRedirectActions, ["retry", "other"]) };
  }
}

/** An email stage's failure with its buttons (plans/accounts-email.md 4, the answers table).
 * `badAddress`, `wrongCode` and `badCode` are a line under the field instead (`emailFieldError`).
 * An empty code is an op with no coded answer: the relay did not answer. */
export function emailFailure(code: string): Failure {
  const expired = acts(w.codeExpiredActions, ["resend", "otherAddress"]);
  switch (code) {
    case "ticketExpired":
    case "wrongFlow":
      return { text: w.codeExpired, actions: expired };
    // Its own words, with the code stage's way on.
    case "signInRefused":
      return { text: w.signInRefused, actions: expired };
    // Send code stays usable: no button of its own.
    case "emailWait":
      return { text: w.emailWait, actions: [] };
    case "emailLimit":
      return { text: w.emailLimit, actions: acts(w.emailLimitActions, ["other"]) };
    case "mailUnavailable":
      return { text: w.mailUnavailable, actions: acts(w.mailUnavailableActions, ["retry", "other"]) };
    // busy, unreachable and the rest: today's words.
    default:
      return signInFailure(code || "unreachable", EMAIL) ?? { text: w.signInRefused, actions: [] };
  }
}

/** The line under an email stage's field, or null when the answer is an `emailFailure`. */
export const emailFieldError = (code: string): string | null => (code === "badAddress" ? w.badAddress : code === "wrongCode" || code === "badCode" ? w.wrongCode : null);

/** A code stage answer that ends its flow: the field stays, read-only, until a new code or another address. */
export const emailEnds = (code: string) => ["ticketExpired", "wrongFlow", "signInRefused", "emailLimit"].includes(code);

/** The code field's value: digits only (a pasted `123 456` or `123-456` works), cut at 6. */
export const codeDigits = (typed: string) => typed.replace(/\D/g, "").slice(0, 6);

/** Seconds until Send a new code turns on: 60 after the last mail, counted from its time. */
export const resendIn = (sentAt: number, now: number) => Math.max(0, Math.ceil((sentAt + 60_000 - now) / 1000));

/** The deletion card's Cancel and turn off <provider> sign-in. */
export const cancelUnlink = (provider: string) => (provider === EMAIL ? w.deletionCancelUnlinkEmail : fill(w.deletionCancelUnlink, { provider: providerName(provider) }));

/** Apple's revoke did not end in a receipt: the unlink or deletion may go on without it (the relay
 * logs that), labelled `anyway`; a cancelled Apple page says nothing. */
export const revokeFailure = (code: string, anyway: string, cancel: string): Failure | null =>
  code === "cancelled"
    ? null
    : {
        text: w.appleRevokeFailed,
        actions: [
          { label: anyway, act: "anyway" },
          { label: cancel, act: "cancel" },
        ],
      };

/** Sign out on a profile and form (D13). The removal is the owner's: a TV's Sign out always
 * removes, so it shows on the owner profile only; elsewhere the owner is asked, ticked by
 * default, and any other profile signs out plainly. */
export function leaveChoice(profile: Profile | null, tv: boolean): { show: boolean; ask: boolean; always: boolean } {
  const owner = !!profile?.isOwner;
  return tv ? { show: owner, ask: false, always: owner } : { show: true, ask: owner, always: false };
}

/** A brokered return names the relay it came from (`relay=<host>`): one that is not the relay
 * this sign-in started on is another service's, never handed to the core. */
export function returnRelay(fragment: string): string | null {
  return new URLSearchParams(fragment).get("relay");
}

export function sameRelay(fragment: string, server: string): boolean {
  const named = returnRelay(fragment);
  if (named === null) return true;
  try {
    return named === new URL(server).hostname;
  } catch {
    return false;
  }
}

type Profile = { isOwner: boolean; kids: boolean };
export const activeProfile = (s: ProfilesSnapshot | null | undefined): Profile | null => current(s) ?? null;

/** A join, a passkey, a recovery kit, or a trim that may have hidden one: a way into the account. */
export const securityKind = (a: Pick<Announcement, "kind">) => a.kind !== "removed";

/** An announcement as a profile shows it: whether it shows at all (a kids profile shows no
 * security notice) and whether it carries OK (only the owner clears a security notice; the core
 * answers `notOwner` otherwise). */
export function shown(a: Pick<Announcement, "kind">, profile: Profile | null): { show: boolean; ok: boolean } {
  if (!profile) return { show: false, ok: false };
  if (!securityKind(a)) return { show: true, ok: true };
  if (profile.kids) return { show: false, ok: false };
  return { show: true, ok: profile.isOwner };
}

/** The name `{device}` stands for: the member `by`, else a member with no name, else gone. */
export function byName(by: string | null, members: readonly MemberInfo[] | null): string {
  const m = by ? members?.find((x) => x.id === by) : undefined;
  if (!m) return w.announceGoneDevice;
  return m.name || w.announceUnnamedDevice;
}

/** An announcement's one line, by the `about` rules. */
export function announcementLine(a: Announcement, members: readonly MemberInfo[] | null): string {
  const device = byName(a.by, members);
  const name = a.name ?? "";
  switch (a.kind) {
    case "device":
      return name ? fill(w.announceDevice, { name }) : w.announceDeviceUnnamed;
    case "passkey":
      return fill(w.announcePasskey, { device });
    case "kit":
      return fill(w.announceKit, { device });
    case "dropped":
      return w.announceDropped;
    default:
      if (a.by !== null && a.by === a.member) return name ? fill(w.announceLeft, { name }) : w.announceLeftUnnamed;
      if (a.passkey) return fill(w.announceRemovedPasskey, { device });
      if (a.kit) return fill(w.announceRemovedKit, { device });
      return name ? fill(w.announceRemoved, { name, device }) : fill(w.announceRemovedUnnamed, { device });
  }
}

/** Home's one account card (4.3), by priority: the scheduled deletion, the account deleted
 * elsewhere, the device that signed this one in (plans/tv-qr-login.md 5.5), the first announcement
 * this profile shows, the nudge for a second way in. */
export type Card =
  | { kind: "deletion"; text: string; provider: string | null }
  | { kind: "gone"; text: string; remove: boolean }
  | { kind: "announcement"; text: string; id: string; ok: boolean; notYou: boolean; member: string | null }
  | { kind: "signedIn"; text: string }
  | { kind: "secondWay"; text: string; detail: string };

export interface CardState {
  deletion: ScheduledDeletion | null;
  gone: boolean;
  announcements: readonly Announcement[];
  members: readonly MemberInfo[] | null;
  profile: Profile | null;
  /** This device syncs and knows the account's members, and none of them is a second way in. */
  alone: boolean;
  /** The device that approved this one's pairing, until OK. */
  signedInFrom: string | null;
  date: (ms: number) => string;
}

export function homeCard(s: CardState): Card | null {
  // A kids profile reaches no account page: it shows no account card at all.
  if (!s.profile || s.profile.kids) return null;
  if (s.deletion) {
    const date = s.date(s.deletion.dueMs);
    const provider = s.deletion.provider;
    if (provider === EMAIL) return { kind: "deletion", text: fill(w.deletionScheduledByEmail, { date }), provider };
    return { kind: "deletion", text: provider ? fill(w.deletionScheduledBy, { provider: providerName(provider), date }) : fill(w.deletionScheduled, { date }), provider };
  }
  if (s.gone) return { kind: "gone", text: w.accountGone, remove: s.profile?.isOwner ?? false };
  if (s.signedInFrom) return { kind: "signedIn", text: fill(w.tvSignedInFrom, { device: s.signedInFrom }) };
  for (const a of s.announcements) {
    const seen = shown(a, s.profile);
    if (seen.show) return { kind: "announcement", text: announcementLine(a, s.members), id: a.id, ok: seen.ok, notYou: a.kind === "passkey" && seen.ok, member: a.member };
  }
  // Kits are the owner's to make (D17): the nudge shows there only.
  if (s.alone && s.profile?.isOwner) return { kind: "secondWay", text: w.secondWayIn, detail: w.secondWayInText };
  return null;
}

/** No second way in yet (4.1.7, the `syncDevices` row): no recovery kit, and no other phone or
 * tablet, the only devices that sign others in. The web saves no passkey (D11). */
export const needsSecondWayIn = (members: readonly MemberInfo[] | null) =>
  !!members && !members.some((m) => m.kit || (!m.thisDevice && (m.form === "phone" || m.form === "tablet")));
