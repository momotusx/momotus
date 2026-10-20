import { describe, expect, test } from "bun:test";
import { type Candidate, pick } from "./spatial";

/** Two rows of three 100x150 tiles, 20 px gaps, second row scrolled so its
 * tiles are offset by 60 px. */
function grid(): Candidate<string>[] {
  const out: Candidate<string>[] = [];
  for (let i = 0; i < 3; i++) {
    out.push({ key: `a${i}`, group: "a", rect: { left: i * 120, top: 0, width: 100, height: 150 } });
    out.push({ key: `b${i}`, group: "b", rect: { left: 60 + i * 120, top: 200, width: 100, height: 150 } });
  }
  return out;
}

const at = (all: Candidate<string>[], key: string) => all.find((c) => c.key === key)!;

describe("pick", () => {
  test("moves along a row and stops at the edge", () => {
    const all = grid();
    const none = new Map<string, string>();
    expect(pick(at(all, "a0"), all, "right", none)).toBe("a1");
    expect(pick(at(all, "a1"), all, "left", none)).toBe("a0");
    expect(pick(at(all, "a2"), all, "right", none)).toBeNull();
    expect(pick(at(all, "a0"), all, "up", none)).toBeNull();
  });

  test("moves down to the best aligned tile", () => {
    const all = grid();
    expect(pick(at(all, "a1"), all, "down", new Map())).toBe("b0");
    expect(pick(at(all, "a2"), all, "down", new Map())).toBe("b1");
  });

  test("entering a stack lands on its first member, moving inside it does not", () => {
    // A tall focused row with two actions stacked to its right; the lower
    // action is nearer the row's centre.
    const all: Candidate<string>[] = [
      { key: "row", rect: { left: 0, top: 0, width: 600, height: 240 } },
      { key: "versions", entry: "versions", rect: { left: 620, top: 10, width: 200, height: 52 } },
      { key: "mark", entry: "versions", rect: { left: 620, top: 70, width: 200, height: 52 } },
      { key: "next", rect: { left: 0, top: 260, width: 600, height: 80 } },
    ];
    const none = new Map<string, string>();
    expect(pick(at(all, "row"), all, "right", none)).toBe("versions");
    expect(pick(at(all, "versions"), all, "down", none)).toBe("mark");
    expect(pick(at(all, "mark"), all, "up", none)).toBe("versions");
    expect(pick(at(all, "mark"), all, "left", none)).toBe("row");
  });

  test("a lone button and a vertical list reach each other sideways; a row's end does not", () => {
    // Versions on a TV: Play at the bottom of the card on the left, a short
    // list of two rows high up on the right. Nothing lies beside either.
    const all: Candidate<string>[] = [
      { key: "play", rect: { left: 113, top: 729, width: 348, height: 75 } },
      { key: "v1", group: "alts", rect: { left: 740, top: 348, width: 1100, height: 64 } },
      { key: "v2", group: "alts", rect: { left: 740, top: 420, width: 1100, height: 64 } },
    ];
    const none = new Map<string, string>();
    // The nearest row, by distance.
    expect(pick(at(all, "play"), all, "right", none)).toBe("v2");
    // From the list (stacked, not a row), Left goes back to Play, also from a
    // focused row that grew a few pixels over its neighbour.
    expect(pick(at(all, "v1"), all, "left", none)).toBe("play");
    const grown = all.map((c) => (c.key === "v2" ? { ...c, rect: { left: 726, top: 410, width: 1128, height: 137 } } : c));
    expect(pick(at(grown, "v2"), grown, "left", none)).toBe("play");
    // A row's end still stops: a2 has b2 to its right, but only in another row.
    const rows = grid();
    expect(pick(at(rows, "a2"), rows, "right", none)).toBeNull();
  });

  test("entering a row restores its remembered tile", () => {
    const all = grid();
    const memory = new Map([["b", "b2"]]);
    expect(pick(at(all, "a0"), all, "down", memory)).toBe("b2");
    // Memory for a tile that is no longer rendered is ignored.
    expect(pick(at(all, "a0"), all, "down", new Map([["b", "gone"]]))).toBe("b0");
  });
});
