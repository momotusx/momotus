import { expect, test } from "bun:test";
import { synopsisBlocks } from "./blocks";

test("a synopsis splits at blank lines, and a long paragraph at its sentence ends", () => {
  expect(synopsisBlocks("One.\n\nTwo.")).toEqual(["One.", "Two."]);
  const sentence = "A sentence of some length that keeps going for a while. ";
  const long = sentence.repeat(20).trim();
  const blocks = synopsisBlocks(long, 200);
  expect(blocks.length).toBeGreaterThan(1);
  expect(blocks.every((b) => b.length <= 200 && b.endsWith("."))).toBe(true);
  expect(blocks.join(" ")).toBe(long);
  // A sentence longer than the limit stays whole: a block is never cut mid-sentence.
  expect(synopsisBlocks("x".repeat(500), 200)).toEqual(["x".repeat(500)]);
});
