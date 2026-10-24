import { expect, test } from "bun:test";
import { qualityBadges } from "./badges";

const words = (b: ReturnType<typeof qualityBadges>) => b.map((x) => x.text);

test("one badge per family, in Apple's order", () => {
  const release = { quality: "4K", features: ["HDR10", "Dolby Vision", "Atmos", "7.1", "Remux", "AD"] };
  expect(words(qualityBadges(release, { languages: 12, hearingImpaired: true }))).toEqual(["4K", "Dolby Vision", "Dolby Atmos", "CC", "SDH", "AD"]);
});

test("HD for a 1080p release, surround when there is no Atmos, nothing the data lacks", () => {
  expect(words(qualityBadges({ quality: "1080p", features: ["5.1"] }, { languages: 0, hearingImpaired: false }))).toEqual(["HD", "5.1"]);
  expect(words(qualityBadges({ quality: "480p", features: [] }, null))).toEqual([]);
  expect(qualityBadges(null, null)).toEqual([]);
});

test("each badge is spoken in full words", () => {
  const spoken = qualityBadges({ quality: "4K", features: ["Atmos", "AD"] }, { languages: 3, hearingImpaired: true }).map((b) => b.spoken);
  expect(spoken).toEqual(["4K", "Dolby Atmos", "Subtitles available", "Subtitles for the deaf and hard of hearing", "Audio description"]);
});
