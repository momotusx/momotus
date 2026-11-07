/** The vault's calls (src/core/vault.ts) on a webOS TV (plans/webos.md section 4, Keystore): the sync record and the
 * profiles' Trakt records live in the app service's keystore, in the app's own data dir, as TV apps keep their
 * tokens. No passkey: a TV has none, and a sign-in on every launch is not acceptable there. Remembering is on from
 * the first join, a start opens what the keystore holds without asking, and the Trakt records are read at start
 * whether or not a sync account is kept (src/core/trakt.ts). */

import type { SyncRecord } from "../lib/sync";
import { keystore } from "./webos-transport";

export { VaultError } from "./vault";

const SYNC = "sync";
const TRAKT = "trakt:";

let opened = false;
const listeners = new Set<() => void>();
const write = (key: string, value: unknown) => keystore({ op: "write", key, value }).then(() => undefined);
const read = () => keystore({ op: "read" });

/** No passkey, so none of the browser's Remember controls. */
export const available = async () => false;
export const stored = async () => SYNC in (await read());
export const active = () => opened;

function open() {
  opened = true;
  for (const fn of listeners) fn();
}

export async function enable(record: SyncRecord): Promise<void> {
  await write(SYNC, record);
  open();
}

export async function unlock(): Promise<SyncRecord> {
  const record = (await read())[SYNC] as SyncRecord | undefined;
  if (!record) throw new Error("the keystore holds no sync record");
  open();
  return record;
}

/** Nothing to confirm: no passkey guards it. */
export const verify = async () => undefined;

export const save = (record: SyncRecord) => (opened ? write(SYNC, record) : Promise.resolve());

/** Forgets the sync record. Closed first and sent at once, so a join right after it lands after the delete; the
 * Trakt records stay (they go with a Trakt sign-out or their profile). */
export function clear(): Promise<void> {
  opened = false;
  return write(SYNC, null);
}

export function drop() {
  opened = false;
}

export const onOpened = (fn: () => void) => void listeners.add(fn);

/** Trakt records stand on their own here: a TV may use Trakt with no sync account. */
export async function saveTrakt(records: Record<string, unknown>): Promise<void> {
  for (const [profile, record] of Object.entries(records)) await write(TRAKT + profile, record);
}

export const forgetTrakt = (profile: string) => write(TRAKT + profile, null);

export const holdsTrakt = async (profile: string) => TRAKT + profile in (await read());

export async function openTrakt(): Promise<Record<string, unknown>> {
  const all = await read();
  return Object.fromEntries(Object.entries(all).flatMap(([key, value]) => (key.startsWith(TRAKT) ? [[key.slice(TRAKT.length), value]] : [])));
}
