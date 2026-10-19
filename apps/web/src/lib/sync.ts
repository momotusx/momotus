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

const param = (code: string, name: string) => {
  try {
    return new URL(code.trim()).searchParams.get(name);
  } catch {
    return null;
  }
};

/** The sync server's host a pairing code names, or null when it names none. */
export function pairingHost(code: string): string | null {
  const s = param(code, "s");
  try {
    return s ? new URL(s).hostname || null : null;
  } catch {
    return null;
  }
}

/** The member id the device showing `code` will have: the first 16 bytes of
 * SHA-256 of its signing key (`g`, base64url), in hex, as the core derives it. */
export async function pairingMember(code: string): Promise<string | null> {
  const g = param(code, "g");
  if (!g) return null;
  let key: Uint8Array<ArrayBuffer>;
  try {
    key = Uint8Array.from(atob(g.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));
  } catch {
    return null;
  }
  if (key.length !== 32) return null;
  const hash = new Uint8Array(await crypto.subtle.digest("SHA-256", key));
  return [...hash.slice(0, 16)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** A link to the core that carries the sync record safely: encrypted, or on
 * this machine. A plain ws:// to another host would show it to the network. */
export function isPrivateLink(url: string): boolean {
  try {
    const u = new URL(url);
    if (u.protocol === "wss:" || u.protocol === "https:") return true;
    const host = u.hostname.replace(/^\[|\]$/g, "");
    return host === "localhost" || host === "::1" || /^127\./.test(host);
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

/** What continuing with the server's list removes: its own drops, and the
 * user's ticks among the server's members (never this device). */
export const forkDrop = (server: readonly MemberInfo[], drops: readonly string[], ticked: ReadonlySet<string>) =>
  server.filter((m) => !m.thisDevice && !drops.includes(m.id) && ticked.has(m.id)).map((m) => m.id);
