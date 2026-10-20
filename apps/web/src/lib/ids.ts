/** Ids for live lists the core sends without one: `liveView` matches array
 * items by `id` across pushes, at every depth, and items without one merge
 * by position (a day inserted above moves every card under the focus).
 * Search rows go by `rowId` (their tiles have an `id`), Calendar days by
 * `date`, their episodes by `videoId` and its series lists by `itemId`. */

import type { CalendarSnapshot } from "../core/generated/CalendarSnapshot";
import type { SearchSnapshot } from "../core/generated/SearchSnapshot";

export const searchIds = (s: SearchSnapshot): SearchSnapshot => ({ ...s, rows: s.rows.map((r) => ({ ...r, id: r.rowId })) });

export const calendarIds = (s: CalendarSnapshot): CalendarSnapshot => ({
  ...s,
  days: s.days.map((d) => ({ ...d, id: d.date, items: d.items.map((i) => ({ ...i, id: i.videoId })) })),
  failed: s.failed.map((x) => ({ ...x, id: x.itemId })),
  noSchedule: s.noSchedule.map((x) => ({ ...x, id: x.itemId })),
});
