/** Profile rules the screens share (plans/m11-profiles.md, P1 apps A and E). */

import type { ProfileInfo } from "../core/generated/ProfileInfo";
import type { ProfilesSnapshot } from "../core/generated/ProfilesSnapshot";

/** Incognito's profile id while it runs (plans/batch-10-incognito.md 2.8): never a real id, which is 32 hex digits. */
export const INCOGNITO = "incognito";

/** Incognito as a profile, for every place that shows the current one. */
export const incognitoProfile: ProfileInfo = { id: INCOGNITO, name: "Incognito", avatar: INCOGNITO, position: 0, hasPin: false, isOwner: false, syncing: false, kids: false };

export const inIncognito = (s: Partial<Pick<ProfilesSnapshot, "incognitoHost">> | null | undefined) => s?.incognitoHost != null;

/** The session's profile: every lookup of the active profile goes through here, so Incognito, which the snapshot never
 * lists, still has a face. Undefined before the core answered, or for a profile removed meanwhile. */
export function current(s: (Pick<ProfilesSnapshot, "profiles" | "active"> & Partial<Pick<ProfilesSnapshot, "incognitoHost">>) | null | undefined): ProfileInfo | undefined {
  if (inIncognito(s)) return incognitoProfile;
  return s?.profiles.find((p) => p.id === s.active);
}

/** The profile a tab remembers it chose (`mq.profileChosen`): never Incognito, which a reload ends. */
export const choiceToRemember = (active: string | undefined): string | null => (active && active !== INCOGNITO ? active : null);

/** At a cold start: the picker when the session is locked or there is more than one profile, else Home. Incognito is
 * never a cold start: it runs only in the session that opened it. */
export function startsOnPicker(s: Pick<ProfilesSnapshot, "locked" | "profiles"> & Partial<Pick<ProfilesSnapshot, "incognitoHost">>): boolean {
  return !inIncognito(s) && (s.locked || s.profiles.length > 1);
}

/** "4:59" for the wait after too many tries, rounded up so it never shows 0:00 while still waiting. */
export function waitText(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}

/** The profile codes the core sends, in words. */
export function profileProblem(code: string | undefined, fallback: string): string {
  switch (code) {
    case "wrongPin":
      return "Wrong PIN";
    case "weakPin":
      return "Pick a less obvious PIN: no repeats, runs or 1234.";
    case "badName":
      return "Names are 1 to 30 characters.";
    case "tooMany":
      return "This account has as many profiles as it can hold.";
    case "notOwner":
      return "Only the main profile can do that.";
    case "viewsOpen":
      return "Something is still open. Try again.";
    case "unknownProfile":
      return "That profile was removed on another device.";
    // The owner's PIN guards the kids profiles: it stays while one exists.
    case "kidsNeedPin":
      return "Kids profiles need this PIN. Remove them first, then remove the PIN.";
    case "pinUnreadable":
      return "This PIN was set by a newer version. Remove it, then set a new one.";
    // A full disk reads as any other failure to open (plans/no-space-warnings.md).
    case "noSpace":
    case "incognitoFailed":
      return "Couldn't open Incognito. Try again.";
    case "incognito":
      return "Not available in Incognito.";
    default:
      return fallback;
  }
}

/** The desktop profile panel's words (plans/profile-panel.md, the owner's pick 7). */
export const panelWords = {
  title: "Profiles",
  close: "Close",
  watchingNow: "Watching now",
  syncing: "Syncing",
  lastWatched: "Last watched",
  switchTo: "Switch to",
  kids: "Kids",
  incognitoLine: "Watch without keeping anything",
  manage: "Manage profiles",
  settings: "Settings",
};

/** What a screen reader hears for a "Switch to" row: the name, then what the row shows in words. A kids session
 * hears that a grown-up's PIN comes next. */
export function panelRowLabel(
  p: Pick<ProfileInfo, "name" | "kids" | "hasPin" | "syncing">,
  spoken: string | undefined,
  kidsSession: boolean,
): string {
  return [
    p.name,
    p.kids ? "kids profile" : "",
    p.syncing ? "still syncing" : p.hasPin ? "locked with a PIN" : "",
    !p.syncing && spoken ? `last watched ${spoken}` : "",
    kidsSession && !p.syncing ? "needs a grown-up's PIN" : "",
  ]
    .filter(Boolean)
    .join(", ");
}
