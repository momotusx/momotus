/** Contrast as drawn: each text's color over the brightest pixel the page paints behind it, with every text,
 * glyph and avatar hidden for the screenshot (WCAG's formula, a translucent color composited over that pixel).
 * Light text over art is at its worst on the brightest pixel; a white still is brighter than any. */

import { writeFileSync } from "node:fs";
import type { Page } from "@playwright/test";

/** A one-pixel white PNG: the brightest still there is, stretched over the frame. */
export const WHITE_PNG = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAAAAAA6fptVAAAACklEQVR4nGP4DwABAQEAsTj2FAAAAABJRU5ErkJggg==", "base64");

export type Measure = { what: string; ratio: number; floor: number };

/** The lowest contrast of each element matched by `texts` (4.5:1) and `glyphs` (3:1) inside `scope`. */
export async function drawnContrast(page: Page, scope: string, texts: string[], glyphs: string[]): Promise<Measure[]> {
  const targets = await page.evaluate(
    ({ scope, texts, glyphs }) => {
      const out: { what: string; x: number; y: number; w: number; h: number; color: string; floor: number }[] = [];
      const add = (sel: string, floor: number) =>
        document.querySelectorAll<HTMLElement>(`${scope} ${sel}`).forEach((el, i) => {
          const s = getComputedStyle(el);
          const color = el.tagName === "svg" || el.closest("svg") ? s.stroke : s.color;
          // Each line of an inline element on its own, 2 px in from its edges: their union also covers the gaps between lines.
          for (const r of el.getClientRects())
            if (r.width > 4 && r.height > 4) out.push({ what: `${sel} ${i}`, x: r.x + 2, y: r.y + 2, w: r.width - 4, h: r.height - 4, color, floor });
        });
      texts.forEach((t) => add(t, 4.5));
      glyphs.forEach((g) => add(g, 3));
      const hide = document.createElement("style");
      hide.id = "contrast-hide";
      hide.textContent = `${scope}, ${scope} * { color: transparent !important; text-shadow: none !important; caret-color: transparent !important; }
        ${scope} svg, ${scope} .avatar, ${scope} img:not(.fade) { visibility: hidden !important; }`;
      document.head.append(hide);
      return out;
    },
    { scope, texts, glyphs },
  );
  const png = await page.screenshot({ animations: "disabled" });
  if (process.env.CONTRAST_SHOT) writeFileSync(`${process.env.CONTRAST_SHOT}-${Date.now()}.png`, png);
  const shot = png.toString("base64");
  return page.evaluate(
    async ({ shot, targets }) => {
      document.getElementById("contrast-hide")?.remove();
      const img = new Image();
      img.src = `data:image/png;base64,${shot}`;
      await img.decode();
      const k = img.naturalWidth / innerWidth;
      const canvas = document.createElement("canvas");
      canvas.width = img.naturalWidth;
      canvas.height = img.naturalHeight;
      const g = canvas.getContext("2d") as CanvasRenderingContext2D;
      g.drawImage(img, 0, 0);
      const lin = (v: number) => (v / 255 <= 0.04045 ? v / 255 / 12.92 : ((v / 255 + 0.055) / 1.055) ** 2.4);
      const lum = (c: number[]) => 0.2126 * lin(c[0]!) + 0.7152 * lin(c[1]!) + 0.0722 * lin(c[2]!);
      return targets.map((t) => {
        const data = g.getImageData(Math.round(t.x * k), Math.round(t.y * k), Math.max(1, Math.round(t.w * k)), Math.max(1, Math.round(t.h * k))).data;
        let bright = [0, 0, 0];
        let most = -1;
        for (let i = 0; i < data.length; i += 4) {
          const c = [data[i]!, data[i + 1]!, data[i + 2]!];
          const l = lum(c);
          if (l > most) [most, bright] = [l, c];
        }
        const [r, gg, b, a = 1] = (t.color.match(/[\d.]+/g) ?? ["0", "0", "0"]).map(Number);
        const fg = [r!, gg!, b!].map((v, i) => v * a + bright[i]! * (1 - a));
        const [x, y] = [lum(fg) + 0.05, most + 0.05];
        return { what: t.what, ratio: Math.round((Math.max(x, y) / Math.min(x, y)) * 100) / 100, floor: t.floor };
      });
    },
    { shot, targets },
  );
}
