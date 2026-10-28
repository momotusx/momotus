// An inert test provider in the Nuvio format (plans/plugins-and-marketplace.md 13.14): it asks its
// own fixture host once, so the host's fetch rule and budget run, then offers the suite's own
// generated test film (colour bars, e2e/fixtures/test-film.webm) for any title. {{BASE}} is the
// fixture host the e2e serves this file from.
const BASE = "{{BASE}}";

async function getStreams(tmdbId, mediaType, season, episode) {
  const answer = await fetch(`${BASE}/lookup/${mediaType}/${tmdbId}`);
  if (!answer.ok) return [];
  const found = await answer.json();
  return [
    {
      name: "Test film",
      title: `${found.title} (test film)`,
      url: `${BASE}/media/test-film.webm`,
      quality: "240p",
    },
  ];
}

module.exports = { getStreams };
