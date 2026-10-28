/** Browser zoom and the desktop canvas scale it feeds (form.ts): pure, so a test can run them without a window. */

/** The scales a display runs at (Windows and macOS: 100 to 400 percent) and the steps a browser's zoom takes. */
const DISPLAY_SCALES = [1, 1.25, 1.5, 1.75, 2, 2.25, 2.5, 3, 3.5, 4];
const ZOOM_STEPS = [1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5];
/** Safari's own steps, between Chromium's. */
const WEBKIT_STEPS = [1.15, 1.85];

/** The browser zoom (a saved site zoom, Ctrl and plus), 1 to 5, from what the page can see: Chromium and WebKit report
 * `outerWidth` unzoomed and `innerWidth` in CSS px, so their ratio is the zoom, and `devicePixelRatio` is the display's
 * scale times it. A ratio counts only when the two agree: DPR over the ratio lands on a display scale (a border or a
 * scroll bar moves it a little) and DPR over that scale is a step the browsers offer. A side panel or docked DevTools
 * widens `outerWidth` and leaves DPR alone, so it fails and the page stays at 1. One case the guard cannot tell apart:
 * Windows at 125 percent display scaling with a side panel of about a fifth of the window reads as 125 percent zoom; the
 * text only gets bigger then, which is harmless. Firefox scales `outerWidth` with the
 * zoom, so it reads 1 there: `desktopScale`'s clamp and the phone form at 720 CSS px are what grow its text. */
export function browserZoom(outer: number, inner: number, dpr: number, webkit = false): number {
  if (!outer || !inner || !dpr || outer / inner < 1.05) return 1;
  const seen = dpr / (outer / inner);
  const base = DISPLAY_SCALES.reduce((a, b) => (Math.abs(b - seen) < Math.abs(a - seen) ? b : a));
  if (Math.abs(base - seen) > 0.03) return 1;
  const zoom = dpr / base;
  return [...ZOOM_STEPS, ...(webkit ? WEBKIT_STEPS : [])].some((z) => Math.abs(z - zoom) < 0.005) ? Math.min(5, zoom) : 1;
}

/** Desktop canvas scale: TV points at arm's length (about 0.63 at 1440 wide). The window's width in CSS px shrinks as
 * the zoom grows, so it is taken at the unzoomed width: the page then grows with the zoom instead of cancelling it. */
export const desktopScale = (width: number, zoom = 1) => Math.min(0.8, Math.max(0.5, (width * zoom) / 2300));
