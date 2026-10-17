// Generates Marquee/Theme/Theme.generated.swift from design/tokens.json.
// Run: bun apps/apple/scripts/gen-theme.ts
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const apple = join(import.meta.dir, "..");
const tokens = JSON.parse(readFileSync(join(apple, "../../design/tokens.json"), "utf8"));

/** "#RRGGBB" or "rgba(r, g, b, a)" to Swift `RGBA(...)` with 0...1 channels. */
function rgba(value: string): string {
  let r: number, g: number, b: number, a = 1;
  const hex = value.match(/^#([0-9a-f]{6})$/i);
  const fn = value.match(/^rgba\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*([\d.]+)\s*\)$/i);
  if (hex) {
    const n = parseInt(hex[1], 16);
    [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  } else if (fn) {
    [r, g, b, a] = [Number(fn[1]), Number(fn[2]), Number(fn[3]), Number(fn[4])];
  } else {
    throw new Error(`unsupported color: ${value}`);
  }
  const c = (v: number) => (v / 255).toFixed(4);
  return `RGBA(${c(r)}, ${c(g)}, ${c(b)}, ${a})`;
}

const fields = (o: Record<string, number>) =>
  Object.entries(o).map(([k, v]) => `${k}: ${v}`).join(", ");

// Dark only (design v2): `color.light` mirrors `color.dark` for the other
// generators; the Apple apps read the dark palette.
const colors = Object.entries(tokens.color.dark as Record<string, string>)
  .map(([k, v]) => `        static let ${k} = Color(${rgba(v)})`)
  .join("\n");

const lets = (o: Record<string, number>) =>
  Object.entries(o).map(([k, v]) => `        static let ${k}: CGFloat = ${v}`).join("\n");
const typeKeys = Object.keys(tokens.type.tv);
const space = lets(tokens.space);
const radius = lets(tokens.radius);
const ease = tokens.motion.ease.match(/cubic-bezier\(([^)]+)\)/)[1].split(",").map((s: string) => Number(s.trim()));

const swift = `// Generated from design/tokens.json by apps/apple/scripts/gen-theme.ts. Do not edit.
import SwiftUI

enum Theme {
    /// The one palette: Marquee is dark only.
    enum Palette {
${colors}
    }

    struct TypeScale: Sendable {
${typeKeys.map((k) => `        let ${k}: CGFloat`).join("\n")}
    }

    static let typeTV = TypeScale(${fields(tokens.type.tv)})
    static let typePhone = TypeScale(${fields(tokens.type.phone)})

    struct Insets: Sendable {
        let top: CGFloat
        let bottom: CGFloat
        let side: CGFloat
    }

    static let safeAreaTV = Insets(${fields(tokens.safeArea.tv)})
    static let safeAreaPhone = Insets(${fields(tokens.safeArea.phone)})

    #if os(tvOS)
    static let type = typeTV
    static let safeArea = safeAreaTV
    #else
    static let type = typePhone
    static let safeArea = safeAreaPhone
    #endif

    enum Space {
${space}
    }

    enum Radius {
${radius}
    }

    enum Focus {
        static let ring: CGFloat = ${tokens.focus.ring}
        /// A soft dark halo this wide around the ring.
        static let halo: CGFloat = ${tokens.focus.halo.dark}
        static let scaleCard: CGFloat = ${tokens.focus.scaleCard}
        static let scaleTile: CGFloat = ${tokens.focus.scaleTile}
    }

    enum Motion {
        static let focus = Animation.timingCurve(${ease.join(", ")}, duration: ${tokens.motion.focusMs / 1000})
        static let panel = Animation.timingCurve(${ease.join(", ")}, duration: ${tokens.motion.panelMs / 1000})
    }

    enum Poster {
        static let tv = CGSize(width: ${tokens.poster.tv.width}, height: ${tokens.poster.tv.height})
        static let landscape = CGSize(width: ${tokens.poster.landscape.width}, height: ${tokens.poster.landscape.height})
    }

    /// Font weights, 100 to 900.
    enum Weight {
${lets(tokens.weight)}
    }

    /// Letter spacing in em: multiply by the font size.
    enum Tracking {
${lets(tokens.tracking)}
    }

    enum Family {
        static let display = "${tokens.font.display[0]}"
        static let text = "${tokens.font.text[0]}"
        static let mono = "${tokens.font.mono[0]}"
    }
}
`;

const out = join(apple, "Marquee/Theme/Theme.generated.swift");
writeFileSync(out, swift);
console.log(`wrote ${out}`);
