import { expect, test } from "bun:test";
import { BridgeError } from "../core/bridge";
import shared from "../../../shared/error-words.json";
import { errorWords, failureWords, kindWords, pairingProblem } from "./words";

// The owner's rule: a failure shows its kind's words, never the core's raw text or a status.
test("a failure shows its kind's words, never its raw text", () => {
  const raw = "network: error sending request for url (http://127.0.0.1:9/manifest.json): Connection refused";
  expect(failureWords(new BridgeError(raw, undefined, undefined, undefined, "network"))).toBe(shared.network);
  expect(failureWords(new BridgeError("http status 404", undefined, undefined, undefined, "notFound"))).toBe(shared.notFound);
  expect(failureWords(new BridgeError("bad request: missing field `url`", undefined, undefined, undefined, "failed"))).toBe(shared.failed);
  // No kind (an older core, or an error from the app itself) is `failed`, never the text.
  expect(failureWords(new BridgeError("internal error: task 7 panicked"))).toBe(shared.failed);
  expect(failureWords(new TypeError("Cannot read properties of undefined"))).toBe(shared.failed);
  expect(failureWords("boom")).toBe(shared.failed);
  // The core's own sentence is kept.
  expect(failureWords(new BridgeError("Kids profiles never run plugins.", undefined, undefined, undefined, "said"))).toBe("Kids profiles never run plugins.");
  // An unknown kind from a newer core, or the file's own note, is `failed`.
  expect(kindWords("somethingNew")).toBe(shared.failed);
  expect(kindWords("about")).toBe(shared.failed);
  for (const [kind, words] of Object.entries(errorWords)) {
    if (kind === "about") continue;
    expect(words).not.toMatch(/\d{3}|http|error:/i);
  }
});

test("sync address failures are read from the kind, never the text", () => {
  expect(pairingProblem("anything", "invalidUrl")).toBe("That is not a sync server address. It looks like https://sync.example.org.");
  expect(pairingProblem("anything", "blockedAddress")).toBe("This device does not connect to that address.");
  expect(pairingProblem("invalid url: https://relay.example")).toBe("The sync server did not answer. Check the address and try again.");
});
