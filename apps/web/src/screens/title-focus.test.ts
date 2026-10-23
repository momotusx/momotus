import { expect, test } from "bun:test";
import { firstFocus } from "./title-focus";

const base = { ready: true, play: false, versions: false, library: false, trailer: false, synopsis: false };

test("TV's first focus on the title falls to the next action when Play is inert or absent", () => {
  expect(firstFocus({ ...base, play: true, versions: true, library: true })).toBe("title:play");
  expect(firstFocus({ ...base, versions: true, library: true })).toBe("title:versions");
  expect(firstFocus({ ...base, library: true, trailer: true })).toBe("title:library");
  // A kids profile (no Library) with no video: the trailer, else the synopsis.
  expect(firstFocus({ ...base, trailer: true, synopsis: true })).toBe("title:trailer");
  expect(firstFocus({ ...base, synopsis: true })).toBe("title:synopsis");
  // Still loading: only a live Play takes it.
  expect(firstFocus({ ...base, ready: false, library: true })).toBeNull();
});
