/** The debrid services Settings offers, equal to Android's `DebridCopy`, and the words around them. */

import type { AccountState } from "../core/generated/AccountState";
import type { DebridLoginSnapshot } from "../core/generated/DebridLoginSnapshot";

export interface DebridService {
  id: string;
  name: string;
  keyPage: string;
}

export const SERVICES: DebridService[] = [
  { id: "realdebrid", name: "Real-Debrid", keyPage: "https://real-debrid.com/apitoken" },
  { id: "torbox", name: "TorBox", keyPage: "https://torbox.app/settings" },
  { id: "alldebrid", name: "AllDebrid", keyPage: "https://alldebrid.com/apikeys" },
  { id: "premiumize", name: "Premiumize", keyPage: "https://www.premiumize.me/account" },
];

/** Services that sign in with a code; the others take a key only. */
export const CODE_SERVICES = new Set(["realdebrid", "alldebrid", "torbox"]);

/** A card's state in words, null when it is not connected. `pending` is a record not stored yet. */
export function stateWords(state: AccountState | undefined, pending: boolean): string | null {
  if (!state) return null;
  if (state === "reconnect") return "Sign in again: the service asked for it";
  if (state === "failing") return "Not answering, trying again";
  return pending ? "Saving" : "Connected";
}

/** What a sign-in by code shows, for every phase but the code itself. */
export function loginWords(phase: DebridLoginSnapshot): string {
  switch (phase.phase) {
    case "starting":
      return "Getting a code";
    case "code":
      return "Enter the code";
    case "done":
      return "Connected";
    case "expired":
      return "The code ran out before it was entered.";
    case "denied":
      return "The sign-in was turned down on the service's page.";
    case "used":
      return "Another sign-in or key took its place.";
    case "failed":
      return phase.reason;
  }
}

/** A sign-in by code is done once its own new connection is stored: the account it replaces
 * (reconnecting) never passes for it before the new record arrives. */
export const signedIn = (phase: string, generation: string | undefined, before: string | undefined, pending: boolean) =>
  phase === "done" && !!generation && generation !== before && !pending;

/** "real-debrid.com/device": the sign-in page as a person types it. */
export const address = (url: string) => url.replace(/^https?:\/\//, "").replace(/^www\./, "").replace(/\/$/, "");

/** How many services are connected, for Settings. */
export const inUse = (count: number) => (count === 0 ? "No debrid service in use." : count === 1 ? "1 debrid service in use." : `${count} debrid services in use.`);
