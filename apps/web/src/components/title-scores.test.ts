import { expect, test } from "bun:test";
import { outLink, scoreItems } from "./title-scores";

test("IMDb reads x/10, TMDB and Trakt as percentages, each spoken once", () => {
  const items = scoreItems("9.3", { tmdb: { state: "shown", text: "87%", url: null }, trakt: { state: "shown", text: "92%", url: null } });
  expect(items.map((i) => [i.mark, i.text, i.spoken])).toEqual([
    ["imdb", "9.3/10", "IMDb 9.3 out of 10"],
    ["tmdb", "87%", "TMDB 87 percent"],
    ["trakt", "92%", "Trakt 92 percent"],
  ]);
});

test("a pending slot keeps its place; a none slot and a missing IMDb score take none", () => {
  const items = scoreItems(null, { tmdb: { state: "none" }, trakt: { state: "pending" } });
  expect(items).toEqual([{ mark: "trakt", text: null, spoken: "", href: null }]);
  // No token in the build: both none from the first render, so nothing is reserved.
  expect(scoreItems(null, { tmdb: { state: "none" }, trakt: { state: "none" } })).toEqual([]);
  // An older core sends no scores: IMDb alone.
  expect(scoreItems("7.4", undefined).map((i) => i.mark)).toEqual(["imdb"]);
});

test("a badge links only to an https page on IMDb, TMDB or Trakt", () => {
  const items = scoreItems("9.3", { tmdb: { state: "shown", text: "87%", url: "https://www.themoviedb.org/movie/278" }, trakt: { state: "shown", text: "92%", url: "https://trakt.tv/movies/tt0111161" } }, "https://www.imdb.com/title/tt0111161/");
  expect(items.map((i) => i.href)).toEqual(["https://www.imdb.com/title/tt0111161/", "https://www.themoviedb.org/movie/278", "https://trakt.tv/movies/tt0111161"]);
  // Anything else is no link: another host, plain http, a script, or no URL (a kids profile gets none).
  for (const bad of ["https://evil.example/tt1", "http://www.imdb.com/title/tt1/", "javascript:alert(1)", "https://trakt.tv.evil.example/x", "", null, undefined]) {
    expect(outLink(bad)).toBeNull();
  }
});
