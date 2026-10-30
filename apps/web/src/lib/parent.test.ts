import { expect, test } from "bun:test";
import { parentOf } from "./parent";
import { isSettingsPage, sectionOfPage } from "./settings-sections";

test("Back with no history walks a Settings page up to its section, then out", () => {
  for (const form of ["phone", "tv", "desktop"] as const) {
    expect(parentOf("/sync/kit", form)).toBe("/sync");
    expect(parentOf("/pair", form)).toBe("/sync");
    expect(parentOf("/sync", form)).toBe("/settings/account");
    expect(parentOf("/profiles/edit/x", form)).toBe("/profiles/manage");
    expect(parentOf("/profiles/new", form)).toBe("/profiles/manage");
    expect(parentOf("/profiles/manage", form)).toBe("/settings");
    for (const page of ["/addons", "/debrid", "/plugins", "/find-addons", "/recommended-addons", "/trakt", "/import"]) expect(parentOf(page, form)).toBe("/settings/sources");
    expect(parentOf("/home-rows", form)).toBe("/settings/home");
    expect(parentOf("/stream-choice", form)).toBe("/settings/playback");
    expect(parentOf("/settings", form)).toBe("/");
    expect(parentOf("/library/downloads/x", form)).toBe("/library/downloads");
    expect(parentOf("/", form)).toBeNull();
  }
  // A section is a page on the phone, a pane beside the menu on TV and desktop.
  for (const path of ["/settings/sources", "/settings/", "/settings/nope"]) {
    expect(parentOf(path, "phone")).toBe("/settings");
    expect(parentOf(path, "tv")).toBe("/");
    expect(parentOf(path, "desktop")).toBe("/");
  }
  expect(parentOf("/title/movie/tt1/versions/tt1", "tv")).toBe("/title/movie/tt1");
});

test("every Settings sub-page but the profile pages has a section", () => {
  for (const page of ["/addons", "/recommended-addons", "/find-addons", "/plugins", "/import", "/pair", "/home-rows", "/stream-choice", "/debrid", "/sync", "/trakt", "/sync/kit"]) {
    expect(isSettingsPage(page)).toBe(true);
    expect(sectionOfPage(page)).not.toBeNull();
  }
  expect(isSettingsPage("/settings/about")).toBe(true);
  expect(sectionOfPage("/profiles/manage")).toBeNull();
  expect(isSettingsPage("/library")).toBe(false);
});
