/** Whether `next` only extends `old` (more pages, the same titles first): then it can show at once, also under TV
 * focus; a list that reorders what is shown waits (plans/discover-v2.md 6.1). */
export function extends_(old: readonly { id: string }[], next: readonly { id: string }[]): boolean {
  return next.length >= old.length && old.every((t, i) => next[i]?.id === t.id);
}
