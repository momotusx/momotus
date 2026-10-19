/** The open profile's Trakt (plans/m7-trakt.md section 3). The `trakt` view
 * stays open while a profile is open. Every record it offers is stored (this
 * tab's memory, and sealed beside the sync record where this browser
 * remembers, src/core/vault.ts) and then acknowledged with `setTrakt`, which
 * goes again before the view reopens after an unlock or a switch. The token
 * never sits in a signal, a log, storage or the URL. */

import { createEffect, createRoot, createSignal, on } from "solid-js";
import { type TraktRecord, TraktRecords } from "../lib/trakt";
import { bridge, op } from "./client";
import type { TraktReport } from "./generated/TraktReport";
import type { TraktStatus } from "./generated/TraktStatus";
import { gateOpen, onProfileReset, profiles } from "./profiles";
import * as vault from "./vault";

const records = new TraktRecords({
  send: (record) => op("setTrakt", { record }).then(() => undefined),
  persist: (all) => vault.saveTrakt(all),
});

const [status, setStatus] = createSignal<TraktStatus | null>(null);
/** The open profile's Trakt state, without its record; null while no profile is open. */
export { status as traktStatus };

type Sub = Awaited<ReturnType<typeof bridge.subscribe>>;
let current: { profile: string; sub: Promise<Sub | null> } | null = null;

function got(profile: string, snapshot: TraktStatus) {
  if (current?.profile !== profile) return;
  const { record, ...rest } = snapshot;
  setStatus({ ...rest, record: null } as TraktStatus);
  if (record) void records.offered(profile, record as TraktRecord).catch(() => undefined);
}

/** `setTrakt` with the profile's record first, then the view. */
function openFor(profile: string) {
  const sub = records
    .resend(profile)
    .catch(() => undefined)
    .then(() =>
      bridge.subscribe<TraktStatus, TraktStatus>(
        "trakt",
        {},
        {
          snapshot: (s) => got(profile, s),
          updates: (updates) => {
            const last = updates[updates.length - 1];
            if (last) got(profile, last);
          },
        },
      ),
    )
    .catch(() => null);
  current = { profile, sub };
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
      () => (gateOpen() ? profiles()?.active : undefined),
      (active) => {
        if (!active || current?.profile === active) return;
        void closeView().then(() => openFor(active));
      },
    ),
  ),
);

// Remembering turned on (a Remember sealed the sync record, or the passkey opened it): the
// records sealed here and the ones in memory meet; the open profile's goes again if it changed.
vault.onOpened(() => {
  void (async () => {
    const profile = current?.profile ?? "";
    const sealed = (await vault.openTrakt().catch(() => ({}))) as Record<string, TraktRecord>;
    if ((await records.restored(sealed, profile)) && current?.profile === profile) await records.resend(profile);
  })().catch(() => undefined);
});

/** "Sync now": one round. */
export const traktSync = () => op<TraktReport>("traktSync");

/** Signs this profile out of Trakt; once the core did, its record goes here too. */
export async function traktSignOut() {
  const profile = current?.profile;
  if (!profile) return;
  await op("traktSignOut");
  await records.signedOut(profile);
}
