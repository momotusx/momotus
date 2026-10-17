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

const colors = Object.keys(tokens.color.dark)
  .map((k) => {
    if (!(k in tokens.color.light)) throw new Error(`light theme lacks ${k}`);
    return `        static let ${k} = Color(light: ${rgba(tokens.color.light[k])}, dark: ${rgba(tokens.color.dark[k])})`;
  })
  .join("\n");

const typeKeys = Object.keys(tokens.type.tv);
const space = Object.entries(tokens.space).map(([k, v]) => `        static let ${k}: CGFloat = ${v}`).join("\n");
const radius = Object.entries(tokens.radius).map(([k, v]) => `        static let ${k}: CGFloat = ${v}`).join("\n");
const ease = tokens.motion.ease.match(/cubic-bezier\(([^)]+)\)/)[1].split(",").map((s: string) => Number(s.trim()));

const swift = `// Generated from design/tokens.json by apps/apple/scripts/gen-theme.ts. Do not edit.
import SwiftUI

enum Theme {
    /// Colors resolve per appearance: light and dark from the token file.
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
        static let halo: CGFloat = ${tokens.focus.halo}
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
