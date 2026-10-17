/** Logos often carry transparent margins that push them off the text column.
 * Like the Apple app, each logo URL is decoded once, its transparent edges are
 * cut away, and the result is kept for the session as a data: URL (the
 * desktop shells' CSP allows data: images). A logo that cannot be read (no
 * CORS) or has nothing to cut is used as it is. */

/** The box around every pixel more opaque than `alpha`, or null when none is. */
export function alphaBox(data: ArrayLike<number>, w: number, h: number, alpha = 8): { left: number; top: number; right: number; bottom: number } | null {
  let top = -1;
  let bottom = -1;
  let left = w;
  let right = -1;
  for (let y = 0; y < h; y++) {
    let row = false;
    for (let x = 0; x < w; x++) {
      if ((data[(y * w + x) * 4 + 3] ?? 0) > alpha) {
        row = true;
        if (x < left) left = x;
        if (x > right) right = x;
      }
    }
    if (row) {
      if (top < 0) top = y;
      bottom = y;
    }
  }
  return top < 0 ? null : { left, top, right, bottom };
}

const cache = new Map<string, Promise<string | null>>();

/** The trimmed logo; the original when it cannot be cut (no CORS, or
 * metahub's occasional duplicated CORS header); null when it does not load
 * at all, so the caller shows the title in words instead of a broken image. */
export function trimmedLogo(url: string): Promise<string | null> {
  let out = cache.get(url);
  if (!out) {
    out = cut(url).catch(() => loads(url));
    cache.set(url, out);
  }
  return out;
}

async function loads(url: string): Promise<string | null> {
  const img = new Image();
  img.src = url;
  try {
    await img.decode();
    return url;
  } catch {
    return null;
  }
}

async function cut(url: string): Promise<string> {
  const img = new Image();
  img.crossOrigin = "anonymous";
  img.src = url;
  await img.decode();
  const w = img.naturalWidth;
  const h = img.naturalHeight;
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) return url;
  ctx.drawImage(img, 0, 0);
  // Throws for an image served without CORS: the caller keeps the original.
  const box = alphaBox(ctx.getImageData(0, 0, w, h).data, w, h);
  if (!box) return url;
  const cw = box.right - box.left + 1;
  const ch = box.bottom - box.top + 1;
  // Margins of a pixel or two are not worth a copy.
  if (cw >= w - 2 && ch >= h - 2) return url;
  const out = document.createElement("canvas");
  out.width = cw;
  out.height = ch;
  out.getContext("2d")?.drawImage(canvas, box.left, box.top, cw, ch, 0, 0, cw, ch);
  return out.toDataURL("image/png");
}
