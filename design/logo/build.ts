// Writes every app's icons, banners, launch art and brand marks from the masters in this folder:
// motmot.svg (the owner's drawing redrawn), motmot-mono.svg, motmot-small.svg and wordmark.svg.
// The icon is the owner's pick 4c (plans/owner-picks.md): the bird at 1.28 of the tile, its top left at
// (-0.20, 0.08), on near-black. Needs rsvg-convert and ImageMagick (brew install librsvg imagemagick).
// Run: bun design/logo/build.ts
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

const here = import.meta.dir;
const root = join(here, "../..");
const BG = "#0B0D0E";
const MINT = "#B3E9C2";

/** 4c: the bird's square at 1.28 of the tile, its top left at (-0.20, 0.08) of the tile. */
export const ART = { scale: 1.28, x: -0.2, y: 0.08 };
/** Points that stay whole (the head's top, the eye's box, the beak's base and tip, the racket and its shaft),
 * in motmot.svg's units, and how far each target shrinks the art about the tile's middle to keep them inside:
 * Android's 66 dp circle (of the 72 dp shown) and a maskable icon's middle 80 percent circle. */
export const KEEP = [[330, 37], [353, 58], [403, 58], [353, 108], [403, 108], [470, 60], [652, 128], [420, 158],
  [655, 146], [694, 190], [621, 170], [632, 330]] as const;
export const ANDROID_K = 0.95;
/** The tvOS front layer's bird: its whole ink (0.978 of its square, from 0.0107 down) at 0.82 of the layer's
 * height, in the middle, so the tilt never shows an edge cut. The square's side and top left in a w x h layer. */
export function tvFront(w: number, h: number) {
  const s = (0.82 * h) / 0.978;
  return { s, x: w / 2 - 0.5 * s, y: (h - 0.978 * s) / 2 - 0.0107 * s };
}
export const MASKABLE_K = 0.83;

const master = readFileSync(join(here, "motmot.svg"), "utf8");
const small = readFileSync(join(here, "motmot-small.svg"), "utf8");
const mono = readFileSync(join(here, "motmot-mono.svg"), "utf8");
const wordmark = readFileSync(join(here, "wordmark.svg"), "utf8");

const viewBox = (svg: string) => svg.match(/viewBox="([^"]+)"/)![1].split(/\s+/).map(Number);
const inner = (svg: string) => svg.replace(/^[\s\S]*?<svg[^>]*>/, "").replace(/<\/svg>\s*$/, "").replace(/<title>[^<]*<\/title>/, "");

/** The bird (its square viewBox) drawn into the box x, y, size. */
function bird(x: number, y: number, size: number, src = master): string {
  return `<svg x="${x}" y="${y}" width="${size}" height="${size}" viewBox="${viewBox(src).join(" ")}">${inner(src)}</svg>`;
}

/** The 4c art in a w x h box: the bird scaled to the box's height, `k` shrinks it about the centre. */
function art(w: number, h: number, k = 1, src = master): string {
  const s = ART.scale * k * h;
  const x = w / 2 + (ART.x - 0.5) * k * h;
  const y = h / 2 + (ART.y - 0.5) * k * h;
  return bird(x, y, s, src);
}

const svg = (w: number, h: number, body: string) =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">${body}</svg>`;
const rect = (w: number, h: number, fill = BG, r = 0) => `<rect width="${w}" height="${h}" rx="${r}" fill="${fill}"/>`;
const clip = (id: string, w: number, h: number, r: number, body: string) =>
  `<defs><clipPath id="${id}"><rect width="${w}" height="${h}" rx="${r}"/></clipPath></defs><g clip-path="url(#${id})">${body}</g>`;

/** The wordmark (ink box) with its height set, its left edge at x and its middle at cy. */
function word(x: number, cy: number, height: number): string {
  const [vx, vy, vw, vh] = viewBox(wordmark);
  const w = (vw / vh) * height;
  return `<svg x="${x}" y="${cy - height / 2}" width="${w}" height="${height}" viewBox="${vx} ${vy} ${vw} ${vh}">${inner(wordmark)}</svg>`;
}
const wordWidth = (height: number) => (viewBox(wordmark)[2] / viewBox(wordmark)[3]) * height;

function run(cmd: string[], input?: string) {
  const p = Bun.spawnSync(cmd, { stdin: input ? Buffer.from(input) : undefined });
  if (p.exitCode !== 0) throw new Error(`${cmd[0]}: ${p.stderr.toString()}`);
}

/** Renders an SVG to a PNG (or, by extension, JPEG or WebP through ImageMagick). `opaque` drops the alpha. */
function render(file: string, w: number, h: number, body: string, opaque = false) {
  mkdirSync(dirname(file), { recursive: true });
  const png = file.replace(/\.(jpg|webp)$/, ".tmp.png");
  run(["rsvg-convert", "-w", String(w), "-h", String(h), "-o", png], body);
  if (png !== file) {
    run(["magick", png, "-strip", ...(file.endsWith(".jpg") ? ["-quality", "90"] : ["-quality", "90", "-define", "webp:method=6"]), file]);
    rmSync(png);
  } else if (opaque) {
    run(["magick", file, "-background", BG, "-alpha", "remove", "-alpha", "off", "-strip", `PNG24:${file}`]);
  }
}

const json = (file: string, data: unknown) => {
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, JSON.stringify(data, null, 2) + "\n");
};
const info = { author: "xcode", version: 1 };

if (import.meta.main) {
  // ---------- Apple: Marquee/Resources/Brand.xcassets ----------
  const xc = join(root, "apps/apple/Marquee/Resources/Brand.xcassets");
  rmSync(xc, { recursive: true, force: true });
  json(join(xc, "Contents.json"), { info });

  // iOS and iPadOS: one full-bleed, opaque 1024 square; the system masks it.
  const appicon = join(xc, "AppIcon.appiconset");
  render(join(appicon, "ios-1024.png"), 1024, 1024, svg(1024, 1024, rect(1024, 1024) + art(1024, 1024)), true);
  // macOS: the macOS 11 grid (an 824 rounded square on a 1024 canvas, with its shadow); 32 px and below draw
  // the small cut, whose heavier lines survive there.
  const macIcon = (n: number) => {
    const u = n / 1024;
    const body = `<defs><filter id="s" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur in="SourceAlpha" stdDeviation="${12 * u}"/>`
      + `<feOffset dy="${10 * u}"/><feComponentTransfer><feFuncA type="linear" slope="0.35"/></feComponentTransfer><feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge></filter></defs>`
      + `<g filter="url(#s)"><g transform="translate(${100 * u} ${100 * u})">${clip("m", 824 * u, 824 * u, 185 * u, rect(824 * u, 824 * u) + art(824 * u, 824 * u, 1, n <= 32 ? small : master))}</g></g>`;
    return svg(n, n, body);
  };
  const macImages = [];
  for (const pt of [16, 32, 128, 256, 512]) {
    for (const scale of [1, 2]) {
      const px = pt * scale;
      const name = `mac-${px}.png`;
      render(join(appicon, name), px, px, macIcon(px));
      macImages.push({ filename: name, idiom: "mac", scale: `${scale}x`, size: `${pt}x${pt}` });
    }
  }
  json(join(appicon, "Contents.json"), {
    images: [{ filename: "ios-1024.png", idiom: "universal", platform: "ios", size: "1024x1024" }, ...macImages],
    info,
  });

  // tvOS: a two-layer image stack, the near-black back and the whole bird in front, 0.82 of the height in the
  // middle: the front layer moves more than the back when the icon tilts, so a bird cut by its edge would show the
  // cut. Then the top shelf.
  const tvBird = (w: number, h: number) => {
    const { s, x, y } = tvFront(w, h);
    return bird(x, y, s);
  };
  const brand = join(xc, "App Icon & Top Shelf Image.brandassets");
  function stack(name: string, w: number, h: number, scales: number[]) {
    const dir = join(brand, `${name}.imagestack`);
    json(join(dir, "Contents.json"), { info, layers: [{ filename: "Front.imagestacklayer" }, { filename: "Back.imagestacklayer" }] });
    for (const [layer, body] of [["Front", (W: number, H: number) => tvBird(W, H)], ["Back", (W: number, H: number) => rect(W, H)]] as const) {
      const set = join(dir, `${layer}.imagestacklayer`);
      json(join(set, "Contents.json"), { info });
      const images = scales.map((s) => {
        const filename = `${layer.toLowerCase()}@${s}x.png`;
        render(join(set, "Content.imageset", filename), w * s, h * s, svg(w * s, h * s, body(w * s, h * s)), layer === "Back");
        return { filename, idiom: "tv", scale: `${s}x` };
      });
      json(join(set, "Content.imageset", "Contents.json"), { images, info });
    }
  }
  stack("App Icon", 400, 240, [1, 2]);
  stack("App Icon - App Store", 1280, 768, [1]);
  function shelf(name: string, w: number, h: number) {
    const dir = join(brand, `${name}.imageset`);
    const images = [1, 2].map((s) => {
      const W = w * s, H = h * s;
      const b = ART.scale * H;
      const wh = 0.13 * H;
      const body = rect(W, H) + bird(W * 0.78 - 0.5 * b, 0.08 * H, b) + word(W * 0.07, H / 2, wh);
      const filename = `${name.toLowerCase().replaceAll(" ", "-")}@${s}x.jpg`;
      render(join(dir, filename), W, H, svg(W, H, body));
      return { filename, idiom: "tv", scale: `${s}x` };
    });
    json(join(dir, "Contents.json"), { images, info });
  }
  shelf("Top Shelf Image", 1920, 720);
  shelf("Top Shelf Image Wide", 2320, 720);
  json(join(brand, "Contents.json"), {
    assets: [
      { filename: "App Icon - App Store.imagestack", idiom: "tv", role: "primary-app-icon", size: "1280x768" },
      { filename: "App Icon.imagestack", idiom: "tv", role: "primary-app-icon", size: "400x240" },
      { filename: "Top Shelf Image Wide.imageset", idiom: "tv", role: "top-shelf-image-wide", size: "2320x720" },
      { filename: "Top Shelf Image.imageset", idiom: "tv", role: "top-shelf-image", size: "1920x720" },
    ],
    info,
  });

  // The bird alone (C and the lockup's mark), the wordmark as a vector, and the launch screen.
  function imageset(name: string, pt: number, src = master) {
    const dir = join(xc, `${name}.imageset`);
    const images = [1, 2, 3].map((s) => {
      const filename = `${name.toLowerCase()}@${s}x.png`;
      const [vx, vy, vw] = viewBox(src);
      render(join(dir, filename), pt * s, pt * s, `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${vx} ${vy} ${vw} ${vw}">${inner(src)}</svg>`);
      return { filename, idiom: "universal", scale: `${s}x` };
    });
    json(join(dir, "Contents.json"), { images, info });
  }
  imageset("Mark", 160);
  imageset("LaunchMark", 120);
  mkdirSync(join(xc, "Wordmark.imageset"), { recursive: true });
  writeFileSync(join(xc, "Wordmark.imageset", "wordmark.svg"), wordmark);
  json(join(xc, "Wordmark.imageset", "Contents.json"), {
    images: [{ filename: "wordmark.svg", idiom: "universal" }],
    info,
    properties: { "preserves-vector-representation": true },
  });
  const rgb = (hex: string) => ({
    "color-space": "srgb",
    components: { alpha: "1.000", red: `0x${hex.slice(1, 3)}`, green: `0x${hex.slice(3, 5)}`, blue: `0x${hex.slice(5, 7)}` },
  });
  // The phone's page is black and the TV's #1C1C1E (design/tokens.json, bgPhone and bg).
  json(join(xc, "LaunchBackground.colorset", "Contents.json"), {
    colors: [{ color: rgb("#000000"), idiom: "universal" }, { color: rgb("#1C1C1E"), idiom: "tv" }],
    info,
  });

  // ---------- Android ----------
  const res = join(root, "apps/android/app/src/main/res");
  // The app's own screens draw the mark and the wordmark from the design module (the TV shell is a library too).
  const designRes = join(root, "apps/android/design/src/main/res");
  const dens = { mdpi: 1, hdpi: 1.5, xhdpi: 2, xxhdpi: 3, xxxhdpi: 4 } as const;
  // The adaptive foreground: 108 dp, the 4c art in the middle 72 dp, shrunk by ANDROID_K so the kept parts stay
  // inside the 66 dp circle no mask cuts; the bird runs on past the 72 dp into the margins the launcher shows
  // when it moves the icon.
  for (const [d, m] of Object.entries(dens)) {
    const n = Math.round(108 * m), v = 72 * m, o = 18 * m;
    const s = ART.scale * ANDROID_K * v;
    const x = o + v / 2 + (ART.x - 0.5) * ANDROID_K * v;
    const y = o + v / 2 + (ART.y - 0.5) * ANDROID_K * v;
    render(join(res, `mipmap-${d}`, "ic_launcher_foreground.png"), n, n, svg(n, n, bird(x, y, s)));
    // Before Android 8 (minSdk 24): the tile itself, rounded as a launcher of that time draws it.
    const l = Math.round(48 * m);
    render(join(res, `mipmap-${d}`, "ic_launcher.png"), l, l, svg(l, l, clip("l", l, l, l * 0.2, rect(l, l) + art(l, l))));
    // The TV banner, 320 x 180 dp at xhdpi: the 4c art at the left, the wordmark at the right.
    const bw = Math.round(160 * m), bh = Math.round(90 * m);
    const ww = 0.39 * bw, wh = (ww / wordWidth(1));
    const banner = rect(bw, bh) + `<svg width="${bh}" height="${bh}" viewBox="0 0 ${bh} ${bh}">${art(bh, bh)}</svg>` + word(bw - ww - 0.044 * bw, bh / 2, wh);
    render(join(res, `drawable-${d}`, "banner.png"), bw, bh, svg(bw, bh, banner), true);
  }
  // Themed icons (Android 13): the whole bird in one colour, 62 dp tall inside the 66 dp circle. An image per
  // density, as lint asks for a path this long; the launcher reads only its alpha.
  for (const [d, m] of Object.entries(dens)) {
    const n = Math.round(108 * m), b = 62 * m;
    render(join(res, `mipmap-${d}`, "ic_launcher_monochrome.png"), n, n, svg(n, n, bird((n - b) / 2, (n - b) / 2, b, mono)));
  }
  // The wordmark as a vector drawable, in mint, its ink box as the viewport.
  {
    const [vx, vy, vw, vh] = viewBox(wordmark);
    const glyphs = [...wordmark.matchAll(/<path transform="translate\(([-\d.]+) ([-\d.]+)\)" d="([^"]+)"\/>/g)];
    if (glyphs.length !== 7) throw new Error(`wordmark.svg: ${glyphs.length} glyphs, expected 7`);
    const paths = glyphs.map(([, x, , d]) => `            <group android:translateX="${x}">\n                <path android:fillColor="${MINT}" android:pathData="${d}" />\n            </group>`).join("\n");
    const h = 24, w = +(h * vw / vh).toFixed(2);
    mkdirSync(join(designRes, "drawable"), { recursive: true });
    writeFileSync(join(designRes, "drawable", "brand_wordmark.xml"),
      `<?xml version="1.0" encoding="utf-8"?>\n<!-- Generated by design/logo/build.ts from wordmark.svg (Unbounded SemiBold, OFL, outlined). -->\n`
      + `<vector xmlns:android="http://schemas.android.com/apk/res/android" android:width="${w}dp" android:height="${h}dp" android:viewportWidth="${vw}" android:viewportHeight="${vh}">\n`
      + `    <group android:translateX="${-vx}" android:translateY="${-vy}">\n        <group android:scaleY="-1">\n${paths}\n        </group>\n    </group>\n</vector>\n`);
  }
  // The bird alone for the app's own screens.
  {
    const [vx, vy, vw] = viewBox(master);
    render(join(designRes, "drawable-nodpi", "brand_mark.webp"), 512, 512, `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${vx} ${vy} ${vw} ${vw}">${inner(master)}</svg>`);
  }

  // ---------- Web ----------
  const pub = join(root, "apps/web/public");
  const tile = (n: number, r = 0, k = 1, src = master) => svg(n, n, r ? clip("t", n, n, r, rect(n, n) + art(n, n, k, src)) : rect(n, n) + art(n, n, k, src));
  render(join(pub, "apple-touch-icon.png"), 180, 180, tile(180), true);
  render(join(pub, "icon-192.png"), 192, 192, tile(192, 192 * 0.2));
  render(join(pub, "icon-512.png"), 512, 512, tile(512, 512 * 0.2));
  // Maskable: the kept parts inside the middle 80 percent circle (k from the icon page's check, 40 percent radius).
  render(join(pub, "icon-maskable-512.png"), 512, 512, tile(512, 0, MASKABLE_K), true);
  render(join(pub, "favicon-32.tmp.png"), 32, 32, tile(32, 6, 1, small));
  render(join(pub, "favicon-16.tmp.png"), 16, 16, tile(16, 3, 1, small));
  run(["magick", join(pub, "favicon-16.tmp.png"), join(pub, "favicon-32.tmp.png"), join(pub, "favicon.ico")]);
  rmSync(join(pub, "favicon-16.tmp.png"));
  rmSync(join(pub, "favicon-32.tmp.png"));
  mkdirSync(join(pub, "brand"), { recursive: true });
  writeFileSync(join(pub, "brand", "wordmark.svg"), wordmark);
  {
    const [vx, vy, vw] = viewBox(master);
    render(join(pub, "brand", "mark.webp"), 384, 384, `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${vx} ${vy} ${vw} ${vw}">${inner(master)}</svg>`);
  }
  console.log("brand assets written");
}
