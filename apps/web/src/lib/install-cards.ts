/** Each Find addons card's Installing and failure state, by entry key. It
 * lives outside the page, so a result that lands after a list switch, or after
 * the page was closed and opened again, updates only its own card. Keys may
 * hold an addon's keys: they stay in memory and are never shown. */

import { createStore } from "solid-js/store";
import { BridgeError } from "../core/bridge";

export type CardState = { busy: boolean; code?: string; words?: string };

const [cards, setCards] = createStore<Record<string, CardState>>({});
export { cards };

/** Runs one card's request; a second press while it runs does nothing.
 * `wordsOf` turns a failure into the words the card shows (`reasonOf`). */
export async function runCard(key: string, request: () => Promise<unknown>, wordsOf: (e: unknown) => string): Promise<void> {
  if (cards[key]?.busy) return;
  setCards(key, { busy: true, code: undefined, words: undefined });
  try {
    await request();
    setCards(key, { busy: false });
  } catch (e) {
    setCards(key, { busy: false, code: e instanceof BridgeError ? e.code : undefined, words: wordsOf(e) });
  }
}
