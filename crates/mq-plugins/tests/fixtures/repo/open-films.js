// A fixture plugin in Nuvio's format (plans/plugins-and-marketplace.md 7.9):
// Blender open movies (CC BY) from the Internet Archive, found through the
// item's metadata, so a run exercises fetch, JSON and the stream read.
const FILMS = {
  // Big Buck Bunny (2008), Blender Foundation, CC BY 3.0.
  tt1254207: "BigBuckBunny_124",
};

async function getStreams(tmdbId, mediaType, season, episode, info) {
  const item = mediaType === "movie" && info && FILMS[info.imdbId];
  if (!item) return [];
  const response = await fetch(`https://archive.org/metadata/${item}`);
  if (!response.ok) return [];
  const meta = await response.json();
  const files = (meta && meta.files) || [];
  return files
    .filter((f) => /\.mp4$/i.test(f.name))
    .map((f) => ({
      name: "Open films",
      title: `${meta.metadata.title} (${f.height || "?"}p)`,
      quality: f.height ? `${f.height}p` : undefined,
      size: f.size ? `${Math.round(f.size / 1048576)} MB` : undefined,
      url: `https://archive.org/download/${item}/${f.name.split("/").map(encodeURIComponent).join("/")}`,
    }));
}

module.exports = { getStreams };
