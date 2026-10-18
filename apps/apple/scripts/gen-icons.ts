// Writes the app's Lucide icons into Marquee/Resources/Icons.xcassets as
// template images that keep their vector data, plus the ISC licence.
// Run: bun apps/apple/scripts/gen-icons.ts <path to an unpacked lucide-static package>
// (npm registry: lucide-static-1.58.0.tgz, sha256 7f37e7f84fcbe3316aa527ba39da937fa8d6ecd085056d47244db19355cdb5b3).
// Names are Lucide's, as the web uses them; "-fill" adds a filled copy.
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const VERSION = "1.58.0";
const ICONS = [
  "arrow-down", "arrow-up", "bookmark-check", "bookmark-plus", "calendar", "calendar-x", "captions", "cat",
  "check", "chevron-down", "chevron-left", "chevron-right", "circle-alert", "circle-check", "circle-dashed",
  "circle-minus", "circle-slash", "circle-x", "clapperboard", "clock", "clock-alert", "compass", "delete",
  "dog", "door-open", "download", "external-link", "eye", "eye-off", "film", "flame", "gamepad-2", "gauge",
  "ghost", "headphones", "history", "house", "inbox", "info", "key-round", "layers", "leaf", "library-big",
  "list-checks", "list-filter", "list-video", "loader-circle", "lock", "log-out", "monitor-check",
  "monitor-smartphone", "moon", "pause", "pause-fill", "pencil", "play", "play-fill", "plus", "popcorn",
  "power", "puzzle", "qr-code", "refresh-cw", "rocket", "rotate-ccw", "rotate-cw", "search", "settings",
  "skip-forward", "sparkle", "star-fill", "text-align-start", "trash", "triangle-alert", "tv", "users",
  "volume-2", "wrench", "x", "zap",
];

const source = process.argv[2];
if (!source) throw new Error("usage: gen-icons.ts <lucide-static package dir>");
const pkg = JSON.parse(readFileSync(join(source, "package.json"), "utf8"));
if (pkg.version !== VERSION) throw new Error(`lucide-static ${pkg.version}, expected ${VERSION}`);

const out = join(import.meta.dir, "../Marquee/Resources/Icons.xcassets");
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
const info = { author: "xcode", version: 1 };
writeFileSync(join(out, "Contents.json"), JSON.stringify({ info }, null, 2) + "\n");

for (const name of ICONS) {
  const filled = name.endsWith("-fill");
  const base = filled ? name.slice(0, -"-fill".length) : name;
  const file = join(source, "icons", `${base}.svg`);
  if (!existsSync(file)) throw new Error(`no Lucide icon named ${base}`);
  // Template rendering takes only the shape; asset SVGs may not honour currentColor.
  let svg = readFileSync(file, "utf8").replaceAll("currentColor", "#000000");
  if (filled) svg = svg.replace('fill="none"', 'fill="#000000"');
  const set = join(out, `${name}.imageset`);
  mkdirSync(set);
  writeFileSync(join(set, `${name}.svg`), svg);
  writeFileSync(
    join(set, "Contents.json"),
    JSON.stringify(
      {
        images: [{ filename: `${name}.svg`, idiom: "universal" }],
        info,
        properties: { "preserves-vector-representation": true, "template-rendering-intent": "template" },
      },
      null,
      2,
    ) + "\n",
  );
}
writeFileSync(join(import.meta.dir, "../Marquee/Resources/LICENSE-lucide.txt"), readFileSync(join(source, "LICENSE"), "utf8"));
console.log(`wrote ${ICONS.length} icons from lucide-static ${VERSION}`);
