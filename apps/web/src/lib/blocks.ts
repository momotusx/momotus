/** A synopsis as blocks the remote can step through (components/synopsis.tsx): its paragraphs, and a
 * paragraph longer than `max` cut at its sentence ends, so no one focus stop runs past the card. */
export function synopsisBlocks(text: string, max = 400): string[] {
  return text
    .split(/\n{2,}/)
    .map((p) => p.trim())
    .filter(Boolean)
    .flatMap((p) => {
      if (p.length <= max) return [p];
      const out: string[] = [];
      for (const s of p.match(/[^.!?]+(?:[.!?]+["')\]]*|$)\s*/g) ?? [p]) {
        const last = out.length - 1;
        if (last >= 0 && out[last]!.length + s.length <= max) out[last] += s;
        else out.push(s);
      }
      return out.map((b) => b.trim()).filter(Boolean);
    });
}
