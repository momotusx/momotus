import { expect, test } from "bun:test";
import { profileProblem, startsOnPicker, waitText } from "./profiles";

const one = [{ id: "default" }];
const two = [{ id: "default" }, { id: "p2" }];

test("the picker opens a cold start only when locked or with more than one profile", () => {
  expect(startsOnPicker({ locked: false, profiles: one as never })).toBe(false);
  expect(startsOnPicker({ locked: true, profiles: one as never })).toBe(true);
  expect(startsOnPicker({ locked: false, profiles: two as never })).toBe(true);
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
