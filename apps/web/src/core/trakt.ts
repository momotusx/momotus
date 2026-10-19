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
/** One opening of the view for one profile; a switch, a lock or a reopen starts another. */
type Session = { profile: string; sub: Promise<Sub | null> };
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

/** The sealed records could not be read since remembering opened: nothing is sealed over them. */
let unread = false;
const records = new TraktRecords({
  send: (record) => op("setTrakt", { record }).then(() => undefined),
  persist: (all) => (unread ? Promise.reject(new Error("the sealed sign-ins were not read")) : vault.saveTrakt(all)),
});

/** An offered record whose store failed: stored and acknowledged once the storage works again. */
let pending: { session: Session; record: TraktRecord } | null = null;

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
  if (record) void store(session, record as TraktRecord);
}

/** The profile's record first (when this page holds one), then the view. */
function openFor(profile: string) {
  const session: Session = { profile, sub: Promise.resolve(null) };
  current = session;
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
        void closeView().then(() => openFor(active));
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
vault.onOpened(() => void meetSealed());

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
  try {
    await records.signedOut(profile);
    return true;
  } catch {
    setStorage({ words: traktWords.deleteFailed, retry: saveAgain });
    return false;
  }
}
