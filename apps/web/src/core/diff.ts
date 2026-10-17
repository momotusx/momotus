import type { ListDiff } from "./generated/ListDiff";

/** Applies one core list diff to an array, returning a new array. Mirrors
 * `ListDiff::apply` in mq-runtime exactly, so UI state equals core state. */
export function applyDiff<T>(list: readonly T[], diff: ListDiff<T>): T[] {
  switch (diff.op) {
    case "append":
      return [...list, ...diff.values];
    case "clear":
      return [];
    case "pushFront":
      return [diff.value, ...list];
    case "pushBack":
      return [...list, diff.value];
    case "popFront":
      return list.slice(1);
    case "popBack":
      return list.slice(0, -1);
    case "insert":
      return [...list.slice(0, diff.index), diff.value, ...list.slice(diff.index)];
    case "set": {
      const next = list.slice();
      next[diff.index] = diff.value;
      return next;
    }
    case "remove":
      return [...list.slice(0, diff.index), ...list.slice(diff.index + 1)];
    case "truncate":
      return list.slice(0, diff.length);
    case "reset":
      return diff.values.slice();
  }
}

export function applyDiffs<T>(list: readonly T[], diffs: ListDiff<T>[]): T[] {
  return diffs.reduce<T[]>((acc, diff) => applyDiff(acc, diff), list.slice());
}
