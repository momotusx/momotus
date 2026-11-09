/** The recommended addons (plans/m13-default-addons.md): what the step, the
 * Settings page and the Home card show, and what Continue sends. */

import type { DefaultAddonInfo } from "../core/generated/DefaultAddonInfo";
import type { DefaultAddons } from "../core/generated/DefaultAddons";
import type { DefaultChoice } from "../core/generated/DefaultChoice";
import { count } from "./words";

/** Group titles, in the order the core lists the groups. */
export const groupTitles: Record<string, string> = {
  catalogs: "Catalogs and details",
  streams: "Streams",
  subtitles: "Subtitles",
  lists: "Lists",
  plugins: "Plugins",
};

export const defaultsWords = {
  title: "Recommended addons",
  lead: "Momotus starts with these. Turn off any you do not want; you can change this later in Settings.",
  review: "Turn off any you do not want; you can change this later in Settings.",
  settingsLede: "The addons Momotus recommends. A switch turned on adds the addon; Addons lists everything you have.",
  noCatalog: "Home and search need a catalog addon.",
  cardTitle: "New recommended addons",
  // How many, never their names: no addon is named on Home (the owner's rule); Review names them.
  cardLine: (n: number) => `Momotus recommends ${count(n, "addon", "addons")} you do not have yet.`,
  tvMore: "Add more later from your phone or computer.",
  // Set up Marquee opens the welcome steps, where the recommended addons are.
  emptyHome: "None of the addons you have on shows a catalog here. Set up Momotus offers the recommended addons, or add an addon yourself.",
  emptyDiscover: "Discover lists the catalogs of the addons you have on, and none of them has one. Set up Momotus offers the recommended addons.",
};

/** The defaults grouped for display: groups in the order they first come,
 * each keeping the core's order (Anime Kitsu joins Cinemeta). */
export function grouped(addons: DefaultAddonInfo[]): { group: string; title: string; addons: DefaultAddonInfo[] }[] {
  const out: { group: string; title: string; addons: DefaultAddonInfo[] }[] = [];
  for (const a of addons) {
    const same = out.find((g) => g.group === a.group);
    if (same) same.addons.push(a);
    else out.push({ group: a.group, title: groupTitles[a.group] ?? a.group, addons: [a] });
  }
  return out;
}

/** The rows of the step that carry a switch: every absent default (a present
 * one shows "From Stremio" or "Installed"); on Review only those, too. */
export const switchable = (addons: DefaultAddonInfo[]) => addons.filter((a) => a.installed === "absent");

/** Continue names exactly the rows that showed a switch, so a default the
 * profile already has keeps its own switch. */
export const choicesFor = (addons: DefaultAddonInfo[], off: ReadonlySet<string>): DefaultChoice[] =>
  switchable(addons).map((a) => ({ key: a.key, on: !off.has(a.key) }));

/** Whether the switches leave the profile with no catalog addon at all. */
export function catalogMissing(addons: DefaultAddonInfo[], off: ReadonlySet<string>): boolean {
  return !addons.some((a) => a.group === "catalogs" && (a.installed === "absent" ? !off.has(a.key) : a.installed === "on"));
}

/** How many addons the Home card offers, or null when it does not show: the
 * profile answered this list version, has every default, or is a kids
 * profile (it reads the owner's list). */
export function cardOffer(defaults: DefaultAddons | null, defaultsSeen: number | undefined, kids: boolean): number | null {
  if (!defaults || defaultsSeen === undefined || kids || defaultsSeen >= defaults.version) return null;
  return switchable(defaults.addons).length || null;
}
