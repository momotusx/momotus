/** Where the page draws its subtitle lines (player.tsx), on every form, as
 * Stremio draws its own (stremio-video withHTMLSubtitles): the same rule as
 * the iPhone and Android (plans/subtitles-letterbox.md). Under the picture at
 * full size when the black band below it holds them (a phone upright); else
 * hung from a foot at the picture's, growing upward, and while the controls
 * show 8 px over the scrubber. Lines that would still meet the transport
 * shrink to 85% while the controls show. Never under a control, never at the
 * top, never under the title-safe line. */

/** The title-safe line, a share of the player's height over its foot (EBU R 95 graphics safe, Android TV's 27 dp of 540). */
export const SAFE_LINE = 0.05;

type Span = { top: number; bottom: number };

export type CueRects = {
  /** The video element. */
  box: { top: number; bottom: number; width: number; height: number };
  /** The picture's own size (videoWidth, videoHeight); 0 before it is known. */
  video: { width: number; height: number };
  /** The controls' block, the scrubber and the transport, when they are in the page. */
  bar?: Span;
  scrubber?: Span;
  transport?: Span;
  /** The controls show. */
  up: boolean;
  /** The lines' height at full size, px. */
  lines: number;
};

/** The lines' box: its top or its foot as px from the element's top or foot, and whether it draws at 85%. */
export function cuePlace({ box, video, bar, scrubber, transport, up, lines }: CueRects): { at: { top?: number; bottom?: number }; shrink: boolean } {
  // The picture inside the element (object-fit: contain).
  const scale = video.width && video.height ? Math.min(box.width / video.width, box.height / video.height) : 0;
  const band = scale ? (box.height - video.height * scale) / 2 : 0;
  const pictureFoot = box.bottom - band;
  const safe = box.bottom - box.height * SAFE_LINE;
  if (band > 0 && Math.min(bar?.top ?? box.bottom, safe) - pictureFoot >= lines + 10) return { at: { top: pictureFoot - box.top + 10 }, shrink: false };
  let foot = Math.min(band > 0 ? pictureFoot - 8 : box.bottom - 16, safe);
  if (up && scrubber) foot = Math.min(foot, scrubber.top - 8);
  return { at: { bottom: box.bottom - foot }, shrink: up && !!transport && foot - lines < transport.bottom && foot > transport.top };
}
