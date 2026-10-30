/** Where Back goes when there is no in-app history (a deep link or reload). */

import type { Form } from "../form";
import { settingsParent } from "./settings-sections";

export function parentOf(path: string, form: Form): string | null {
  const parts = path.split("/").filter(Boolean);
  if (parts[0] === "title" && parts[3] === "versions") return `/${parts.slice(0, 3).join("/")}`;
  if (parts[0] === "play" && parts[2]) return `/title/${parts[1]}/${parts[2]}`;
  if (path.startsWith("/profiles/edit/") || path === "/profiles/new") return "/profiles/manage";
  if (path.startsWith("/sync/") || path === "/pair" || path === "/sign-in") return "/sync";
  if (path.startsWith("/sign-in/")) return "/sign-in";
  if (path.startsWith("/library/downloads/")) return "/library/downloads";
  if (path.startsWith("/library/")) return "/library";
  return settingsParent(path, form) ?? (path === "/" ? null : "/");
}
