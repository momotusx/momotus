import { expect, test } from "bun:test";
import { BridgeError } from "../core/bridge";
import { cards, forgetAllCards, runCard as run } from "./install-cards";

const words = (e: unknown) => (e instanceof Error ? e.message : String(e));
const runCard = (key: string, request: () => Promise<unknown>) => run(key, request, words);

test("a late result updates only its own card, and a second press waits", async () => {
  let finish: (v: unknown) => void = () => undefined;
  let calls = 0;
  const slow = () => {
    calls++;
    return new Promise((r) => (finish = r));
  };
  const first = runCard("a", slow);
  void runCard("a", slow);
  expect(calls).toBe(1);
  expect(cards.a?.busy).toBe(true);
  await runCard("b", () => Promise.reject(new BridgeError("This addon now needs setup first", "needsSetup")));
  expect(cards.b).toEqual({ busy: false, code: "needsSetup", words: "This addon now needs setup first" });
  // The page closes and opens again: the store outlives it, and the late
  // answer lands on its own card.
  finish({ id: "x", name: "X" });
  await first;
  expect(cards.a).toEqual({ busy: false });
  expect(cards.b?.code).toBe("needsSetup");
});

test("a new profile forgets every card, and a request from before lands nowhere", async () => {
  let finish: (v: unknown) => void = () => undefined;
  const running = runCard("p", () => new Promise((r) => (finish = r)));
  await runCard("q", () => Promise.reject(new BridgeError("Not installed", "notListed")));
  forgetAllCards(false);
  expect(cards.p?.busy).toBe(true);
  expect(cards.q).toBeUndefined();
  forgetAllCards();
  expect(cards.p).toBeUndefined();
  finish({});
  await running;
  expect(cards.p).toBeUndefined();
});
