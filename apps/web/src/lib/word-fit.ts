/** The longest run of whole words of `text`, then an ellipsis, that `fits`; `text` itself when it fits or is one
 * word (the line's own ellipsis cuts that one where it must). A tile's caption line ends this way ("Game of...",
 * never "Game of Thr...": the browser's own ellipsis cuts inside a word). */
export function wordFit(text: string, fits: (line: string) => boolean): string {
  if (fits(text)) return text;
  const words = text.split(" ").filter(Boolean);
  for (let n = words.length - 1; n >= 1; n--) {
    // A separator left at the end ("S2:E3 ·") goes with the words after it.
    const cut = `${words.slice(0, n).join(" ").replace(/[\s·,:;-]+$/u, "")}…`;
    if (fits(cut)) return cut;
  }
  return text;
}
