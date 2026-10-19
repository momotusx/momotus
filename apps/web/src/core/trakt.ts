/** The open profile's Trakt (plans/m7-trakt.md section 3). The `trakt` view
 * stays open while a profile is open. Every record it offers is stored (this
 * tab's memory, and sealed beside the sync record where this browser
 * remembers, src/core/vault.ts) and only then acknowledged with `setTrakt`.
 * Before the view opens (at start, after an unlock or a switch) the profile's
 * record goes again, if this page holds one; a page that holds none sends
 * nothing, so a reload or a second tab never ends a live connection. Each
 * send is bound to the session (profile and opening) it started under. No
 * record travels a link that is not private, and the token never sits in a
 * signal, a log, storage in the clear or the URL. */

import { createEffect, createRoot, createSignal, on } from "solid-js";
import { type TraktRecord, TraktRecords, traktWords } from "../lib/trakt";
import { bridge, op, privateLink } from "./client";
import type { TraktReport } from "./generated/TraktReport";
import type { TraktStatus } from "./generated/TraktStatus";
import { gateOpen, onProfileReset, profiles } from "./profiles";
import * as vault from "./vault";

type Sub = Awaited<ReturnType<typeof bridge.subscribe>>;

/** Work started here and not waited for by its caller (a store, an opening, a delete). */
const busy = new Set<Promise<unknown>>();
function track(work: Promise<unknown>) {
  busy.add(work);
  const done = () => void busy.delete(work);
  void work.then(done, done);
}
/** Settles once nothing started here is still running, however long that takes (for tests). */
export async function traktSettled() {
  while (busy.size) await Promise.allSettled([...busy]);
}
/** One opening of the view for one profile; a switch, a lock or a reopen starts another.
 * `offered` names the last record it took, so a record pushed again is stored once. */
type Session = { profile: string; sub: Promise<Sub | null>; offered?: string };
let current: Session | null = null;
const still = (session: Session) => () => current === session;

const [status, setStatus] = createSignal<TraktStatus | null>(null);
/** The open profile's Trakt state, without its record; null while no profile is open. */
export { status as traktStatus };

/** What this browser's storage did wrong with the Trakt sign-in, and the way to try again. */
const [storage, setStorage] = createSignal<{ words: string; retry: () => Promise<void> } | null>(null);
export { storage as traktStorage };

/** False on a page whose link to the core is not private: no Trakt record travels it. */
export const traktLink = privateLink;

/** This browser keeps the open profile's sign-in under a passkey not opened yet: unlocking on
 * Devices and sync brings it back. */
const [lockedHere, setLockedHere] = createSignal(false);
export { lockedHere as traktLockedHere };

/** The sealed records could not be read since remembering opened: nothing is sealed over them. */
let unread = false;
const records = new TraktRecords({
  send: (record) => op("setTrakt", { record }).then(() => undefined),
  persist: (all) => (unread ? Promise.reject(new Error("the sealed sign-ins were not read")) : vault.saveTrakt(all)),
  forget: (profile) => vault.forgetTrakt(profile),
});

/** An offered record whose store failed: stored and acknowledged once the storage works again. */
let pending: { session: Session; record: TraktRecord } | null = null;

/** The profile's record is deleted (signed out, or the profile is gone): a store still waiting for
 * it must never bring it back, and its note goes with it. */
function dropPending(profile: string) {
  if (pending?.session.profile !== profile) return;
  pending = null;
  if (storage()?.words === traktWords.saveFailed) setStorage(null);
}

/** Stores an offered record and acknowledges it within its session. A store that failed is said
 * (a sealed copy that could not be read first, as the cause) and nothing is acknowledged. */
async function store(session: Session, record: TraktRecord) {
  try {
    await records.offered(session.profile, record, still(session));
    if (pending?.record === record) pending = null;
    if (storage()?.words === traktWords.saveFailed) setStorage(null);
  } catch {
    pending = { session, record };
    if (current === session && !unread) setStorage({ words: traktWords.saveFailed, retry: saveAgain });
  }
}

function got(session: Session, snapshot: TraktStatus) {
  if (current !== session) return;
  const { record, ...rest } = snapshot;
  setStatus({ ...rest, record: null } as TraktStatus);
  if (!record) return;
  // Every push carries the record until it is acknowledged: one store and one acknowledgement each.
  const r = record as TraktRecord;
  const name = `${r.account}:${r.generation}:${r.revision}`;
  if (session.offered === name) return;
  session.offered = name;
  track(store(session, r));
}

/** The profile's record first (when this page holds one), then the view. */
function openFor(profile: string) {
  const session: Session = { profile, sub: Promise.resolve(null) };
  current = session;
  setLockedHere(false);
  if (!vault.active())
    track(
      vault.holdsTrakt(profile).then(
        (held) => current === session && setLockedHere(held),
        () => undefined,
      ),
    );
  session.sub = records
    .resend(profile, still(session))
    .catch(() => undefined)
    .then(() =>
      current !== session
        ? null
        : bridge.subscribe<TraktStatus, TraktStatus>(
            "trakt",
            {},
            {
              snapshot: (s) => got(session, s),
              updates: (updates) => {
                const last = updates[updates.length - 1];
                if (last) got(session, last);
              },
            },
          ),
    )
    .then(async (sub) => {
      // Overtaken while it opened (two switches in a row): it closes again.
      if (sub && current !== session) {
        await sub.close();
        return null;
      }
      return sub;
    })
    .catch(() => null);
  track(session.sub);
}

async function closeView() {
  const was = current;
  current = null;
  setStatus(null);
  await was?.sub.then((s) => s?.close()).catch(() => undefined);
}

// A switch or a lock closes it (the switch waits for that); an open profile opens it again.
onProfileReset(closeView);
createRoot(() =>
  createEffect(
    on(
      () => (traktLink && gateOpen() ? profiles()?.active : undefined),
      (active) => {
        if (!active || current?.profile === active) return;
        // Two changes in a row: only the newest opens, once.
        track(
          closeView().then(() => {
            if (current === null && gateOpen() && profiles()?.active === active) openFor(active);
          }),
        );
      },
    ),
  ),
);

/** Remembering turned on (a Remember sealed the sync record, or the passkey opened it): the records
 * sealed here and the ones in memory meet, and the open profile's goes again if it changed. A read
 * that failed seals nothing over what is there and says so. */
async function meetSealed() {
  const session = current;
  let sealed: Record<string, TraktRecord>;
  setLockedHere(false);
  try {
    sealed = (await vault.openTrakt()) as Record<string, TraktRecord>;
    unread = false;
  } catch {
    unread = true;
    setStorage({ words: traktWords.readFailed, retry: meetSealed });
    return;
  }
  try {
    const changed = await records.restored(sealed, session?.profile ?? "");
    if (storage()?.words === traktWords.readFailed) setStorage(null);
    if (session && changed) await records.resend(session.profile, still(session));
  } catch {
    setStorage({ words: traktWords.saveFailed, retry: saveAgain });
    return;
  }
  if (pending) await store(pending.session, pending.record);
}
vault.onOpened(() => track(meetSealed()));

// A profile removed here or by sync: its record goes from memory and its sealed copy from this
// browser (no key needed). A delete that fails is tried again: the core names removed profiles
// again when the next session starts.
bridge.onProfileGone((profile) => {
  dropPending(profile);
  track(records.signedOut(profile).catch(() => undefined));
});

/** Stores what memory holds again, then acknowledges a record still waiting; the note goes once
 * it worked. */
async function saveAgain() {
  if (pending) return store(pending.session, pending.record);
  await records.save();
  setStorage(null);
}

/** "Sync now": one round. */
export const traktSync = () => op<TraktReport>("traktSync");

/** Signs this profile out of Trakt; once the core did, its record goes here too. False when a
 * sealed copy could not be deleted: that is said, with Try again. */
export async function traktSignOut(): Promise<boolean> {
  const profile = current?.profile;
  if (!profile) return false;
  await op("traktSignOut");
  dropPending(profile);
  // Gone from this browser too, its lock open or not: never "signed out" with a sealed copy left.
  const forget = async () => {
    await records.signedOut(profile);
    setLockedHere(false);
    if (storage()?.words === traktWords.deleteFailed) setStorage(null);
  };
  try {
    await forget();
    return true;
  } catch {
    setStorage({ words: traktWords.deleteFailed, retry: forget });
    return false;
  }
}
