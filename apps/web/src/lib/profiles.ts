/** Profile rules the screens share (plans/m11-profiles.md, P1 apps A and E). */

import type { ProfilesSnapshot } from "../core/generated/ProfilesSnapshot";

/** At a cold start: the picker when the session is locked or there is more than one profile, else Home. */
export function startsOnPicker(s: Pick<ProfilesSnapshot, "locked" | "profiles">): boolean {
  return s.locked || s.profiles.length > 1;
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
    case "pinUnreadable":
      return "This PIN was set by a newer version. Remove it, then set a new one.";
    default:
      return fallback;
  }
}
