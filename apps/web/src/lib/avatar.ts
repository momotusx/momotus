/** Profile avatars (plans/m11-profiles.md, P1 apps D): the register stores
 * `glyph:color`, both ids from these fixed lists. An id this app does not
 * know draws the fallback, so a newer app's avatar never breaks this one. */

export const GLYPHS = ["letter", "popcorn", "clapperboard", "tv", "rocket", "ghost", "cat", "dog", "leaf", "moon", "flame", "headphones", "gamepad"] as const;
export const COLORS = ["ember", "amber", "moss", "teal", "sky", "plum", "rose", "graphite"] as const;

export type Glyph = (typeof GLYPHS)[number];
export type AvatarColor = (typeof COLORS)[number];
export type Avatar = { glyph: Glyph; color: AvatarColor | null };

const isGlyph = (s: string): s is Glyph => (GLYPHS as readonly string[]).includes(s);
const isColor = (s: string): s is AvatarColor => (COLORS as readonly string[]).includes(s);

/** A stored avatar. Never chosen (empty): the letter on ember. An unknown part falls back to the
 * letter, or to `raised` (color null) for an unknown color. */
export function parseAvatar(stored: string): Avatar {
  if (stored === "") return { glyph: "letter", color: "ember" };
  const [g = "", c = ""] = stored.split(":");
  return { glyph: isGlyph(g) ? g : "letter", color: isColor(c) ? c : null };
}

export const formatAvatar = (a: { glyph: Glyph; color: AvatarColor }) => `${a.glyph}:${a.color}`;

// Older TV browsers have no Segmenter: they draw the first code point instead.
const graphemes = typeof Intl.Segmenter === "function" ? new Intl.Segmenter(undefined, { granularity: "grapheme" }) : null;

/** The name's first grapheme (a letter with its accents, a joined emoji), as the letter avatar draws it. */
export function initial(name: string): string {
  const text = name.trim();
  const first = graphemes ? graphemes.segment(text)[Symbol.iterator]().next().value?.segment : [...text][0];
  return first ? first.toLocaleUpperCase() : "?";
}

/** A new profile's avatar: the letter, on the first color no profile uses yet (else the next in turn). */
export function nextAvatar(existing: string[]): string {
  const used = new Set(existing.map((s) => parseAvatar(s).color));
  const color: AvatarColor = COLORS.find((c) => !used.has(c)) ?? COLORS[existing.length % COLORS.length] ?? "ember";
  return formatAvatar({ glyph: "letter", color });
}
