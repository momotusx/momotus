import { expect, test } from "bun:test";
import { startsOnPicker, waitText } from "./profiles";

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
