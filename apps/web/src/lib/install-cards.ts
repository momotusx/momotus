/** Each Find addons card's Installing and failure state, by entry key. It
 * lives outside the page, so a result that lands after a list switch, or after
 * the page was closed and opened again, updates only its own card. Keys may
 * hold an addon's keys: they stay in memory and are never shown. */

import { createStore, reconcile } from "solid-js/store";
import { BridgeError } from "../core/bridge";

export type CardState = { busy: boolean; code?: string; words?: string };

const [cards, setCards] = createStore<Record<string, CardState>>({});
export { cards };

// Bumped when every card is forgotten for a new profile: a request still
// running from before must not land on the new profile's card.
let generation = 0;

/** Forgets a card's last result (its entry's installed state changed). */
export const forgetCard = (key: string) => {
  if (cards[key] && !cards[key].busy) setCards(key, { busy: false });
};

/** Forgets every card's result; for a new profile also those still running. */
export function forgetAllCards(running = true) {
  if (running) generation++;
  setCards(reconcile(Object.fromEntries(Object.entries(cards).filter(([, s]) => !running && s.busy))));
}

/** Runs one card's request; a second press while it runs does nothing.
 * `wordsOf` turns a failure into the words the card shows (`reasonOf`). */
export async function runCard(key: string, request: () => Promise<unknown>, wordsOf: (e: unknown) => string): Promise<void> {
  if (cards[key]?.busy) return;
  const mine = generation;
  setCards(key, { busy: true, code: undefined, words: undefined });
  try {
    await request();
    if (mine === generation) setCards(key, { busy: false });
  } catch (e) {
    if (mine === generation) setCards(key, { busy: false, code: e instanceof BridgeError ? e.code : undefined, words: wordsOf(e) });
  }
}
