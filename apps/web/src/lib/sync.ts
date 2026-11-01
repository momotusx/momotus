/** Sync rules the screens share (plans/core-protocol.md, the sync ops). */

import type { MemberInfo } from "../core/generated/MemberInfo";
import type { ProfilesSnapshot } from "../core/generated/ProfilesSnapshot";
import { current } from "./profiles";
import { syncWords as w } from "./words";

const onOwner = (s: ProfilesSnapshot | null | undefined) => current(s)?.isOwner === true;

/** Delete account shows on the owner profile only: the account holds every profile (D15). */
export const mayDeleteAccount = onOwner;

/** Make a recovery kit (and, later, Save a passkey) shows on the owner profile only: a kit is a
 * portable copy of the whole account key (D17). */
export const mayMakeKit = onOwner;

/** The keystore record (`SyncConfig`): opaque to the app but for its revision. */
export type SyncRecord = Record<string, unknown>;

const revision = (r: SyncRecord) => (typeof r.revision === "number" ? r.revision : 0);

/** Records carry a revision and the app keeps the highest: an answer that
 * arrives after a newer record was stored never rolls it back. */
export const keepNewer = (held: SyncRecord | null, next: SyncRecord): SyncRecord => (held && revision(held) > revision(next) ? held : next);

/** A pairing code as the new device shows it: the TV's link `https://<link host>/pair#v=3&...`,
 * or `marquee:pair?v=2&...` from an older build. Any other QR in view is ignored. */
export function isPairingCode(text: string): boolean {
  const code = text.trim();
  if (code.startsWith("marquee:pair?")) return true;
  try {
    const link = new URL(code);
    return link.protocol === "https:" && link.pathname === "/pair" && new URLSearchParams(link.hash.slice(1)).get("v") === "3";
  } catch {
    return false;
  }
}

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
  return s ? memberName(s) : "a device no longer in your account";
}

/** What a lost report drops: the lost device and what it added that the user did not untick. */
export const lostDrop = (lost: string, added: readonly string[], keep: ReadonlySet<string>) => [lost, ...added.filter((id) => !keep.has(id))];

/** The kits that go with a lost report, from the core's answer: the preview's kits, less the
 * ones a device the user unticked made (they stay with it). */
export function kitsGoing(preview: { kits: readonly string[]; deviceKits: readonly { device: string; kits: readonly string[] }[] }, keep: ReadonlySet<string>): string[] {
  const staying = new Set(preview.deviceKits.filter((d) => keep.has(d.device)).flatMap((d) => d.kits));
  return preview.kits.filter((k) => !staying.has(k));
}

const param = (code: string, name: string) => {
  try {
    return new URL(code.trim()).searchParams.get(name);
  } catch {
    return null;
  }
};

/** A link to the core that carries the sync record safely: encrypted, or on
 * this machine. A plain ws:// to another host would show it to the network. */
export function isPrivateLink(url: string): boolean {
  try {
    const u = new URL(url);
    if (u.protocol === "wss:" || u.protocol === "https:") return true;
    const host = u.hostname.replace(/^\[|\]$/g, "");
    // The whole host as an IPv4 literal in 127.0.0.0/8, never a name that starts with 127.
    const v4 = /^127\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(host);
    return host === "localhost" || host === "::1" || (!!v4 && v4.slice(1).every((n) => Number(n) <= 255));
  } catch {
    return false;
  }
}

/** Looks for a pairing code until one is seen or `stopped`. A code seen by a
 * look that ends after the stop is dropped: a cancelled scan approves nothing. */
export async function watchForCode(
  detect: () => Promise<string[]>,
  stopped: () => boolean,
  found: (code: string) => void,
  pause: (ms: number) => Promise<void> = (ms) => new Promise((r) => setTimeout(r, ms)),
): Promise<void> {
  while (!stopped()) {
    const seen = await detect().catch(() => [] as string[]);
    if (stopped()) return;
    const code = seen.find(isPairingCode);
    if (code) return found(code.trim());
    await pause(250);
  }
}

/** The rows of a lost report: every device it drops by default, with names
 * from the list when known, else as an unknown device (never hidden). */
export function lostRows(preview: readonly string[], members: readonly MemberInfo[]): { id: string; member: MemberInfo | null }[] {
  return preview.map((id) => ({ id, member: members.find((m) => m.id === id) ?? null }));
}

/** The two sides of a different device list, by id: on both, only on the server, only here. */
export function forkSides(mine: readonly MemberInfo[], server: readonly MemberInfo[]) {
  const here = new Set(mine.map((m) => m.id));
  const there = new Set(server.map((m) => m.id));
  return {
    both: server.filter((m) => here.has(m.id)),
    serverOnly: server.filter((m) => !here.has(m.id)),
    mineOnly: mine.filter((m) => !there.has(m.id)),
  };
}

/** The kits that go with the ticked devices of the server's list, besides its own drops. */
export function forkKits(kits: readonly { device: string; kits: string[] }[], ticked: readonly string[], drops: readonly string[]): string[] {
  return kits.filter((k) => ticked.includes(k.device)).flatMap((k) => k.kits.filter((id) => !drops.includes(id)));
}

/** What continuing with the server's list removes: its own drops, and the
 * user's ticks among the server's members (never this device). */
export const forkDrop = (server: readonly MemberInfo[], drops: readonly string[], ticked: ReadonlySet<string>) =>
  server.filter((m) => !m.thisDevice && !drops.includes(m.id) && ticked.has(m.id)).map((m) => m.id);
