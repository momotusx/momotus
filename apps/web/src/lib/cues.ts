/** Where the page draws its subtitle lines (player.tsx), on every form, in the
 * page's own px (player.tsx undoes the canvas scale). Under the picture at full
 * size when the black band below it holds them on an upright screen (a phone);
 * else hung from a foot 8% of the picture's height over the picture's foot
 * (Media3's own default on its content frame; every platform's, plans/dual-subtitles.md
 * section 11), growing upward, and while the controls show 8 px over the scrubber.
 * Lines that would still meet the transport shrink to 85% while the controls show; with a strip open, lines that
 * would meet it even at 85% go over it.
 * Never under a control, never at the top, never under the title-safe line, never
 * on the band of a landscape screen. */

/** The title-safe line, a share of the player's height over its foot (EBU R 95 graphics safe, Android TV's 27 dp of 540). */
export const SAFE_LINE = 0.05;

/** The foot's height over the picture's foot, a share of the picture's height. */
export const FOOT = 0.08;

type Span = { top: number; bottom: number };

export type CueRects = {
  /** The video element. */
  box: { top: number; bottom: number; width: number; height: number };
  /** The picture's own size (videoWidth, videoHeight); 0 before it is known. */
  video: { width: number; height: number };
  /** The controls' block, the scrubber and the transport, when they are in the page. */
  bar?: Span;
  /** The top bar (the title block), for a second line hung from the top (Split, plans/dual-subtitles.md 4). */
  top?: Span;
  scrubber?: Span;
  transport?: Span;
  /** The open subtitle or audio strip over the bar (plans/player-bar.md 5): the lines keep 8 px over it. */
  strip?: Span;
  /** The controls show. */
  up: boolean;
  /** The lines' height at full size, px. */
  lines: number;
  /** The profile's position (Prefs.subtitlePosition, 5 to 25): the foot's height over the picture's foot, a percent
   * of the picture's height; 8, the default, is FOOT. */
  position?: number;
};

/** The lines' box: its top or its foot as px from the element's top or foot, and whether it draws at 85%. With
 * the controls hidden the foot sits `position` percent of the picture over its foot, never under the 5% line; with
 * them up it is today's lift (8 px over the scrubber and the open strip) raised by the position's share over 8, so a
 * change shows live. The band of an upright screen ignores the position (plans/player-bar.md D7). */
export function cuePlace({ box, video, bar, scrubber, transport, strip, up, lines, position = FOOT * 100 }: CueRects): { at: { top?: number; bottom?: number }; shrink: boolean } {
  // The picture inside the element (object-fit: contain).
  const scale = video.width && video.height ? Math.min(box.width / video.width, box.height / video.height) : 0;
  const picture = scale ? video.height * scale : box.height;
  const band = (box.height - picture) / 2;
  const pictureFoot = box.bottom - band;
  const safe = box.bottom - box.height * SAFE_LINE;
  // The band takes the lines only upright: on a landscape screen it is a thin bar, and a cue that fits it while the
  // next does not would jump between the bar and the picture.
  if (box.height > box.width && band > 0 && Math.min(bar?.top ?? box.bottom, safe) - pictureFoot >= lines + 10) return { at: { top: pictureFoot - box.top + 10 }, shrink: false };
  const share = position / 100;
  let foot: number;
  if (up && (scrubber || strip)) {
    foot = Math.min(pictureFoot - picture * FOOT, safe);
    if (scrubber) foot = Math.min(foot, scrubber.top - 8);
    if (strip) foot = Math.min(foot, strip.top - 8);
    foot -= picture * Math.max(0, share - FOOT);
    // An open strip (with its Style row most of all) can leave no room under a transport in the screen's middle
    // (a phone across): the lines go over it, 8 px clear, and stay readable while the viewer tunes them (Pixel walk
    // 2026-11-09 L23), instead of landing under Back 10, Play and Forward 10.
    if (strip && transport && foot > transport.top && foot - lines * 0.85 < transport.bottom) foot = transport.top - 8;
  } else foot = Math.min(pictureFoot - picture * share, safe);
  return { at: { bottom: box.bottom - foot }, shrink: up && !!transport && foot - lines < transport.bottom && foot > transport.top };
}

/** Where Split's second line hangs from the top (plans/dual-subtitles.md 4 B): its top edge, px from the element's
 * top, the mirror of the foot. On a landscape screen 8% of the picture's height under the picture's top; on an
 * upright one in the band over the picture when it holds the block (its lines and 10 px), never over the 5% line.
 * Under the top bar while the controls show, and under the lowest sign the first line's file placed in the top
 * half (`signs`), 8 px clear of each. */
export function cueTop({ box, video, top, up, lines, signs }: CueRects & { signs: Span[] }): number {
  const scale = video.width && video.height ? Math.min(box.width / video.width, box.height / video.height) : 0;
  const picture = scale ? video.height * scale : box.height;
  const pictureTop = box.top + (box.height - picture) / 2;
  const safe = box.top + box.height * SAFE_LINE;
  const inBand = Math.max(safe, pictureTop - lines - 10);
  const start = box.height > box.width && pictureTop - safe >= lines + 10 ? inBand : Math.max(pictureTop + picture * FOOT, safe);
  const sign = Math.max(0, ...signs.filter((s) => s.top < box.top + box.height / 2).map((s) => s.bottom + 8));
  return Math.max(start, up && top ? top.bottom + 8 : 0, sign) - box.top;
}

/** The settings a WebVTT cue carries (VTTCue's own). */
export type CueSettings = { line: number | "auto"; snapToLines: boolean; position: number | "auto"; align: string; size: number };

/** Where a cue its file placed draws, over the element, as the browser drew it; null for a cue that joins the
 * foot block (no line, a line counted from the foot, or one at 90% or lower, as Android's SSA_FOOT). Rule 1 of plans/subtitles-letterbox.md: a sign at
 * line:10% stays at the top. Its top: a share of the element (line:10%), or `down` lines of the cue's own
 * height under the element's top (line:2). Across: its box `size` wide, at `position` (else from its align). */
export function cueSpot({ line, snapToLines, position, align, size }: CueSettings): { top: string; down: number; left: string; width: string; align: "flex-start" | "center" | "flex-end" } | null {
  if (line === "auto" || (snapToLines ? line < 0 : line >= 90)) return null;
  const side = align === "left" || align === "start" ? "start" : align === "right" || align === "end" ? "end" : "center";
  const at = position === "auto" ? { start: 0, center: 50, end: 100 }[side] : position;
  const left = Math.min(Math.max(at - { start: 0, center: size / 2, end: size }[side], 0), 100 - size);
  return {
    top: snapToLines ? "0%" : `${line}%`,
    down: snapToLines ? line : 0,
    left: `${left}%`,
    width: `${size}%`,
    align: side === "start" ? "flex-start" : side === "end" ? "flex-end" : "center",
  };
}
