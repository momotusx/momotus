/** Which tiles of a row to mount: those overlapping the visible span plus a
 * buffer each side, always including the focused tile's neighbors so spatial
 * focus can step onto a tile before the scroll catches up. */
export function visibleRange(
  scroll: number,
  viewport: number,
  stride: number,
  count: number,
  focused: number,
  buffer = 3,
): [start: number, end: number] {
  if (count <= 0 || stride <= 0) return [0, 0];
  let start = Math.floor(scroll / stride) - buffer;
  let end = Math.ceil((scroll + viewport) / stride) + buffer;
  if (focused >= 0) {
    start = Math.min(start, focused - buffer);
    end = Math.max(end, focused + buffer + 1);
  }
  return [Math.max(0, start), Math.min(count, end)];
}
