// Generates Marquee/Theme/Theme.generated.swift from design/tokens.json.
// Run: bun apps/apple/scripts/gen-theme.ts; with --check it writes nothing and exits 1 when the file is stale.
import { existsSync, readFileSync, writeFileSync } from "node:fs";
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
// generators; the Apple apps read the dark palette. Per form (D10): where the tokens name a phone or
// a desktop value (`bgPhone`, `bgDesktop`), iOS and macOS take it under the base name; tvOS keeps
// the base.
const dark = tokens.color.dark as Record<string, string>;
const colors = Object.entries(dark)
  .map(([k, v]) => {
    const phone = dark[`${k}Phone`];
    const desktop = dark[`${k}Desktop`];
    const line = (value: string) => `        static let ${k} = Color(${rgba(value)})`;
    if (!phone && !desktop) return line(v);
    // Only the branches that differ from the base.
    const arms = [phone && ["#if os(iOS)", line(phone)], desktop && [phone ? "#elseif os(macOS)" : "#if os(macOS)", line(desktop)]].filter(Boolean) as string[][];
    return [...arms.flatMap(([cond, l]) => [`        ${cond}`, l]), "        #else", line(v), "        #endif"].join("\n");
  })
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
        /// A selected segment or chip: one grey step up (\`selected.fill\`).
        static let selectedFill = Color(${rgba(tokens.selected.fill)})
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
        /// The Home hero carousel (the owner's B2): a page's time, its slide, and its art's speed against its words.
        static let heroDwell: Double = ${tokens.motion.heroDwellMs / 1000}
        static let heroSlide = Animation.timingCurve(${ease.join(", ")}, duration: ${tokens.motion.heroSlideMs / 1000})
        static let heroParallax: CGFloat = ${tokens.motion.heroParallax}
    }

    enum Poster {
        static let tv = CGSize(width: ${tokens.poster.tv.width}, height: ${tokens.poster.tv.height})
        static let landscape = CGSize(width: ${tokens.poster.landscape.width}, height: ${tokens.poster.landscape.height})
    }

    /// The phone's tiles (\`tile.phone\`), read by \`Metrics.compact\`.
    enum Tile {
        static let poster = CGSize(width: ${tokens.tile.phone.poster.width}, height: ${tokens.tile.phone.poster.height})
        static let landscape = CGSize(width: ${tokens.tile.phone.landscape.width}, height: ${tokens.tile.phone.landscape.height})
        static let gap: CGFloat = ${tokens.tile.phone.gap}
        /// The phone page's side inset (\`tile.phone.inset\`).
        static let inset: CGFloat = ${tokens.tile.phone.inset}
        /// A poster's corners, then a 16:9 card's.
        static let radius: CGFloat = ${tokens.tile.phone.radius}
        static let cardRadius: CGFloat = ${tokens.tile.phone.cardRadius}
        /// Discover's grid (\`tile.phone.grid\`): the screen's width shared by the columns, height by \`aspect\`.
        enum Grid {
            static let columns = ${tokens.tile.phone.grid.columns}
            static let aspect: CGFloat = ${tokens.tile.phone.grid.aspect}
            static let gap: CGFloat = ${tokens.tile.phone.grid.gap}
            static let rowGap: CGFloat = ${tokens.tile.phone.grid.rowGap}
        }
    }

    /// The Mac's shelves (\`tile.desktop\`, tv.apple.com's), for \`Metrics.regular\`.
    enum TileDesktop {
        static let poster = CGSize(width: ${tokens.tile.desktop.poster.width}, height: ${tokens.tile.desktop.poster.height})
        static let landscape = CGSize(width: ${tokens.tile.desktop.landscape.width}, height: ${tokens.tile.desktop.landscape.height})
        static let gap: CGFloat = ${tokens.tile.desktop.gap}
        static let inset: CGFloat = ${tokens.tile.desktop.inset}
        static let radius: CGFloat = ${tokens.tile.desktop.radius}
        /// Discover's grid (\`tile.desktop.grid\`).
        enum Grid {
            static let poster = CGSize(width: ${tokens.tile.desktop.grid.poster.width}, height: ${tokens.tile.desktop.grid.poster.height})
            static let gap: CGFloat = ${tokens.tile.desktop.grid.gap}
            static let rowGap: CGFloat = ${tokens.tile.desktop.grid.rowGap}
        }
    }

    /// Search's Top results (\`search\`): a card's smallest size and its plain poster.
    enum Search {
        static let topCard = CGSize(width: ${tokens.search.topCard.minWidth}, height: ${tokens.search.topCard.minHeight})
        static let thumb = CGSize(width: ${tokens.search.thumb.width}, height: ${tokens.search.thumb.height})
    }

    /// A pull-down pill's height (\`control.pill\`).
    enum Pill {
        static let desktop: CGFloat = ${tokens.control.pill.desktop}
        static let phone: CGFloat = ${tokens.control.pill.phone}
    }

    /// Discover's preview panel (\`preview\`): its width, its gap from the grid, its corners.
    enum Preview {
        static let width: CGFloat = ${tokens.preview.width}
        static let gap: CGFloat = ${tokens.preview.gap}
        static let radius: CGFloat = ${tokens.preview.radius}
    }

    /// Scrims laid over art: \`hover\` is tv.apple.com's grey over a card under the pointer.
    enum Scrim {
        static let hover = Color(${rgba(tokens.scrim.hover)})
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
if (process.argv.includes("--check")) {
  const fresh = existsSync(out) && readFileSync(out, "utf8") === swift;
  console.log(fresh ? `${out} matches design/tokens.json` : `${out} is stale: run bun apps/apple/scripts/gen-theme.ts`);
  process.exit(fresh ? 0 : 1);
}
writeFileSync(out, swift);
console.log(`wrote ${out}`);
