/** Sync rules the screens share (plans/core-protocol.md, the sync ops). */

import type { MemberInfo } from "../core/generated/MemberInfo";
import { syncWords as w } from "./words";

/** The keystore record (`SyncConfig`): opaque to the app but for its revision. */
export type SyncRecord = Record<string, unknown>;

const revision = (r: SyncRecord) => (typeof r.revision === "number" ? r.revision : 0);

/** Records carry a revision and the app keeps the highest: an answer that
 * arrives after a newer record was stored never rolls it back. */
export const keepNewer = (held: SyncRecord | null, next: SyncRecord): SyncRecord => (held && revision(held) > revision(next) ? held : next);

/** A pairing code as the new device shows it; any other QR in view is ignored. */
export const isPairingCode = (text: string) => text.trim().startsWith("marquee:pair?");

/** "VW4BV9DJD5EZ" to "VW4B V9DJ D5EZ", for reading aloud. */
export const fingerprintGroups = (fp: string) => fp.match(/.{1,4}/g)?.join(" ") ?? fp;

/** This device first, then the other devices and then the kits, oldest first. */
export const sortMembers = (members: readonly MemberInfo[]) =>
  [...members].sort((a, b) => Number(b.thisDevice) - Number(a.thisDevice) || Number(a.kit) - Number(b.kit) || a.addedMs - b.addedMs);

export const memberName = (m: Pick<MemberInfo, "kit" | "name">) => (m.kit ? w.kitName : m.name || w.unnamed);

/** The name of the device that added `m`, or null for the first device. */
export function sponsorName(m: Pick<MemberInfo, "sponsor">, members: readonly MemberInfo[]): string | null {
  if (!m.sponsor) return null;
  const s = members.find((x) => x.id === m.sponsor);
  return s ? memberName(s) : "a removed device";
}

/** What a lost report drops: the lost device and what it added that the user did not untick. */
export const lostDrop = (lost: string, added: readonly string[], keep: ReadonlySet<string>) => [lost, ...added.filter((id) => !keep.has(id))];

/** The kits that go with a lost report, as the core works them out: every
 * kit of the preview whose maker does not stay (a device the user unticked
 * keeps its kits). */
export function kitsGoing(previewKits: readonly string[], members: readonly MemberInfo[], drop: readonly string[]): string[] {
  const ids = new Set(members.map((m) => m.id));
  return previewKits.filter((k) => {
    const maker = members.find((m) => m.id === k)?.sponsor;
    return !maker || !ids.has(maker) || drop.includes(maker);
  });
}
