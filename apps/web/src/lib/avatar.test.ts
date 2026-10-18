import { expect, test } from "bun:test";
import { initial, nextAvatar, parseAvatar } from "./avatar";

test("a stored avatar parses, and anything unknown falls back to the letter", () => {
  expect(parseAvatar("popcorn:amber")).toEqual({ glyph: "popcorn", color: "amber" });
  expect(parseAvatar("hologram:amber")).toEqual({ glyph: "letter", color: "amber" });
  expect(parseAvatar("popcorn:ultraviolet")).toEqual({ glyph: "popcorn", color: null });
  expect(parseAvatar("")).toEqual({ glyph: "letter", color: "ember" });
  expect(parseAvatar(":")).toEqual({ glyph: "letter", color: null });
});

test("the letter is the name's first character, and a new profile takes an unused color", () => {
  expect(initial("  élise")).toBe("É");
  expect(initial("")).toBe("?");
  expect(nextAvatar([])).toBe("letter:ember");
  expect(nextAvatar(["cat:ember", "letter:amber"])).toBe("letter:moss");
  expect(nextAvatar([""])).toBe("letter:amber");
  const all = ["ember", "amber", "moss", "teal", "sky", "plum", "rose", "graphite"].map((c) => `letter:${c}`);
  expect(nextAvatar(all)).toBe("letter:ember");
});
