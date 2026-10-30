/** Settings in sections on A1 (plans/settings-sections.md, "On A1"): the sections each open their own page on the
 * phone and their own pane beside the menu on TV and desktop. Which section each Settings sub-page belongs to, and
 * where Back goes from one. */

import type { Form } from "../form";

export type SectionId = "sources" | "account" | "home" | "playback" | "downloads" | "device" | "about";

export const SECTION_IDS: readonly SectionId[] = ["sources", "account", "home", "playback", "downloads", "device", "about"];

export const sectionId = (id: string | undefined): SectionId | undefined => SECTION_IDS.find((s) => s === id);

/** Settings sub-pages: they keep the rail and light up Settings. */
const SETTINGS_PAGES = ["/settings", "/addons", "/recommended-addons", "/find-addons", "/plugins", "/import", "/pair", "/home-rows", "/stream-choice", "/debrid", "/sync", "/trakt"];

export const isSettingsPage = (path: string) =>
  SETTINGS_PAGES.includes(path) || path.startsWith("/settings/") || path.startsWith("/profiles/") || path.startsWith("/sync/");

const PAGE_SECTION: Record<string, SectionId> = {
  "/addons": "sources",
  "/recommended-addons": "sources",
  "/find-addons": "sources",
  "/plugins": "sources",
  "/debrid": "sources",
  "/trakt": "sources",
  "/import": "sources",
  "/sync": "account",
  "/pair": "account",
  "/home-rows": "home",
  "/stream-choice": "playback",
};

/** The section a Settings sub-page is opened from; null for anything else (the profile pages sit under the header). */
export const sectionOfPage = (path: string): SectionId | null => (path.startsWith("/sync/") ? "account" : (PAGE_SECTION[path] ?? null));

/** Where Back goes from a Settings page with no in-app history: a sub-page to its section; a section to the top
 * level on the phone, or Home where a section is a pane beside the menu; the profile pages to Settings. Null for
 * pages outside Settings. */
export function settingsParent(path: string, form: Form): string | null {
  if (path.startsWith("/settings/")) return form === "phone" ? "/settings" : "/";
  const section = sectionOfPage(path);
  if (section) return `/settings/${section}`;
  return path.startsWith("/profiles/") ? "/settings" : null;
}
