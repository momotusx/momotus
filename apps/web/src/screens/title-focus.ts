/** The title page's first focus on TV: a live Play; else (Play inert, or no video yet) the next action
 * there is, so the remote is never left without a focus. Nothing until the header is ready, so an early
 * action never takes the focus from a Play still on its way. */
export type FirstFocus = "title:play" | "title:versions" | "title:library" | "title:trailer" | "title:synopsis" | null;

export function firstFocus(p: { ready: boolean; play: boolean; versions: boolean; library: boolean; trailer: boolean; synopsis: boolean }): FirstFocus {
  if (p.play) return "title:play";
  if (!p.ready) return null;
  if (p.versions) return "title:versions";
  if (p.library) return "title:library";
  if (p.trailer) return "title:trailer";
  return p.synopsis ? "title:synopsis" : null;
}
