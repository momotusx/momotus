import { expect, test } from "bun:test";
import { choiceToRemember, current, INCOGNITO, inIncognito, profileProblem, startsOnPicker, waitText } from "./profiles";

const one = [{ id: "default" }];
const two = [{ id: "default" }, { id: "p2" }];

test("the picker opens a cold start only when locked or with more than one profile", () => {
  expect(startsOnPicker({ locked: false, profiles: one as never })).toBe(false);
  expect(startsOnPicker({ locked: true, profiles: one as never })).toBe(true);
  expect(startsOnPicker({ locked: false, profiles: two as never })).toBe(true);
});

test("Incognito never starts on the picker: it runs only in the session that opened it", () => {
  expect(startsOnPicker({ locked: false, profiles: two as never, incognitoHost: "default" })).toBe(false);
  expect(startsOnPicker({ locked: false, profiles: two as never, incognitoHost: null })).toBe(true);
});

test("the current profile is Incognito's own face while it runs, never a listed profile", () => {
  const listed = [{ id: "default", name: "Ana", avatar: "cat:moss", position: 0, hasPin: false, isOwner: true, syncing: false, kids: false }];
  expect(current({ profiles: listed, active: "default", incognitoHost: null })?.name).toBe("Ana");
  const face = current({ profiles: listed, active: INCOGNITO, incognitoHost: "default" });
  expect(face).toEqual({ id: INCOGNITO, name: "Incognito", avatar: INCOGNITO, position: 0, hasPin: false, isOwner: false, syncing: false, kids: false });
  expect(current({ profiles: listed, active: "gone", incognitoHost: null })).toBeUndefined();
  expect(current(null)).toBeUndefined();
  expect(inIncognito({ incognitoHost: "default" })).toBe(true);
  expect(inIncognito({ incognitoHost: null })).toBe(false);
});

test("the tab never remembers Incognito as its chosen profile", () => {
  expect(choiceToRemember(INCOGNITO)).toBeNull();
  expect(choiceToRemember("default")).toBe("default");
  expect(choiceToRemember(undefined)).toBeNull();
});

test("Incognito's codes read as words, with no space words for a full disk", () => {
  expect(profileProblem("noSpace", "x")).toBe("Couldn't open Incognito. Try again.");
  expect(profileProblem("incognitoFailed", "x")).toBe("Couldn't open Incognito. Try again.");
  expect(profileProblem("incognito", "x")).toBe("Not available in Incognito.");
});

test("the wait reads as minutes and seconds and never shows zero while waiting", () => {
  expect(waitText(299_000)).toBe("4:59");
  expect(waitText(60_000)).toBe("1:00");
  expect(waitText(1)).toBe("0:01");
  expect(waitText(0)).toBe("0:00");
  expect(waitText(3_600_000)).toBe("60:00");
});

test("the core's profile codes read as words, and an unknown one keeps its own message", () => {
  expect(profileProblem("pinUnreadable", "x")).toBe("This PIN was set by a newer version. Remove it, then set a new one.");
  expect(profileProblem("weakPin", "x")).toBe("Pick a less obvious PIN: no repeats, runs or 1234.");
  expect(profileProblem("somethingNew", "the core's text")).toBe("the core's text");
});
