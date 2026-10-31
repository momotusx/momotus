import { expect, test } from "bun:test";
import { wordFit } from "./word-fit";

const fit = (text: string, chars: number) => wordFit(text, (line) => line.length <= chars);

test("a caption line that fits stays whole", () => {
  expect(fit("Dune", 10)).toBe("Dune");
});

test("a long caption line ends at the last whole word that fits, never inside one", () => {
  expect(fit("Game of Thrones", 11)).toBe("Game of…");
  expect(fit("S2:E3 · 36 min left", 16)).toBe("S2:E3 · 36 min…");
  // A separator left at the end goes with the words after it.
  expect(fit("S2:E3 · 36 min left", 8)).toBe("S2:E3…");
});

test("a first word too long for the line is left to the line's own ellipsis", () => {
  expect(fit("Supercalifragilistic", 5)).toBe("Supercalifragilistic");
  expect(fit("Supercalifragilistic film", 5)).toBe("Supercalifragilistic film");
});
