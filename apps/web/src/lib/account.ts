/** Accounts on the web (plans/accounts.md 4.1 to 4.3, 5): the words of every
 * sign-in state and card, and the rules that pick them, kept apart from the
 * screens so both can be tested. Every word comes from
 * `apps/shared/account-words.json`, the file the apps read too; its `about`
 * holds the rules this file follows. */

import shared from "../../../shared/account-words.json";
import type { Announcement } from "../core/generated/Announcement";
import type { MemberInfo } from "../core/generated/MemberInfo";
import type { ProfilesSnapshot } from "../core/generated/ProfilesSnapshot";
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

/** The order of a relay's providers on the first screen: Apple, Google and Facebook first, as
 * the web shows no platform's own sheet (4.1.1), then any other in the relay's order. */
export const providerOrder = (ids: readonly string[]) => [...ids.filter((id) => id in w.providerName).sort((a, b) => Object.keys(w.providerName).indexOf(a) - Object.keys(w.providerName).indexOf(b)), ...ids.filter((id) => !(id in w.providerName))];

/** What a button under a failure does: sign in again with the same provider, the other ways,
 * Not now, move the sign-in to this account, go on without Apple's receipt, or put the line away. */
export type Act = "retry" | "other" | "notNow" | "move" | "anyway" | "cancel";
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
      return { text: fill(w.signupLimit, { providerAccount: account }), actions: acts(w.signupLimitActions, ["other"]) };
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
    return named === new URL(server).host;
  } catch {
    return false;
  }
}

type Profile = { isOwner: boolean; kids: boolean };
export const activeProfile = (s: ProfilesSnapshot | null | undefined): Profile | null => s?.profiles.find((p) => p.id === s.active) ?? null;

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
 * elsewhere, the first announcement this profile shows, the nudge for a second way in. */
export type Card =
  | { kind: "deletion"; text: string; provider: string | null }
  | { kind: "gone"; text: string; remove: boolean }
  | { kind: "announcement"; text: string; id: string; ok: boolean; notYou: boolean }
  | { kind: "secondWay"; text: string; detail: string };

export interface CardState {
  deletion: ScheduledDeletion | null;
  gone: boolean;
  announcements: readonly Announcement[];
  members: readonly MemberInfo[] | null;
  profile: Profile | null;
  /** This device syncs, knows the account's members, and only this device and no kit are in it. */
  alone: boolean;
  date: (ms: number) => string;
}

export function homeCard(s: CardState): Card | null {
  if (s.deletion) {
    const date = s.date(s.deletion.dueMs);
    const provider = s.deletion.provider;
    return { kind: "deletion", text: provider ? fill(w.deletionScheduledBy, { provider: providerName(provider), date }) : fill(w.deletionScheduled, { date }), provider };
  }
  if (s.gone) return { kind: "gone", text: w.accountGone, remove: s.profile?.isOwner ?? false };
  for (const a of s.announcements) {
    const seen = shown(a, s.profile);
    if (seen.show) return { kind: "announcement", text: announcementLine(a, s.members), id: a.id, ok: seen.ok, notYou: a.kind === "passkey" && seen.ok };
  }
  // Kits are the owner's to make (D17): the nudge shows there only.
  if (s.alone && s.profile?.isOwner) return { kind: "secondWay", text: w.secondWayIn, detail: w.secondWayInText };
  return null;
}

/** Only this device, and no kit: one lost device locks the account (4.1.7). */
export const aloneInAccount = (members: readonly MemberInfo[] | null) => !!members && !members.some((m) => m.kit) && members.filter((m) => !m.kit).length <= 1;
