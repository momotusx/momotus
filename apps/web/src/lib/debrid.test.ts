import { expect, test } from "bun:test";
import { address, inUse, loginWords, stateWords } from "./debrid";

test("a card says connected only once its record is stored", () => {
  expect(stateWords(undefined, false)).toBeNull();
  expect(stateWords("ok", false)).toBe("Connected");
  expect(stateWords("ok", true)).toBe("Saving");
  expect(stateWords("reconnect", false)).toBe("Sign in again: the service asked for it");
  expect(stateWords("failing", true)).toBe("Not answering, trying again");
});

test("every sign-in phase has its words, and addresses read as typed", () => {
  expect(loginWords({ phase: "starting" })).toBe("Getting a code");
  expect(loginWords({ phase: "used" })).toBe("Another sign-in or key took its place.");
  expect(loginWords({ phase: "failed", reason: "no network" })).toBe("no network");
  expect(address("https://real-debrid.com/device/")).toBe("real-debrid.com/device");
  expect(inUse(0)).toBe("No debrid service in use.");
  expect(inUse(2)).toBe("2 debrid services in use.");
});
