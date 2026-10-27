"""The Archive index rules of plans/plugins-and-marketplace.md 5.2, measured on
one `services/search/v1/scrape` answer (fields identifier,title,year,licenseurl,
downloads). Usage: uv run python -I archive-index-rules.py <answer.json>"""

import json
import re
import sys
from collections import Counter

NOISE = r"\b(full movie|full hd|hd|restored|english|dubbed|subtitled|colorized)\b"
# Latin letters with accents as their plain ones, as the core folds them.
FOLDS = {"àáâãäåāăą": "a", "çćĉċč": "c", "ďđ": "d", "èéêëēĕėęě": "e", "ĝğġģ": "g", "ĥħ": "h",
         "ìíîïĩīĭįı": "i", "ĵ": "j", "ķ": "k", "ĺļľŀł": "l", "ñńņňŉ": "n", "òóôõöøōŏő": "o",
         "ŕŗř": "r", "śŝşš": "s", "ţťŧ": "t", "ùúûüũūŭůűų": "u", "ŵ": "w", "ýÿŷ": "y", "źżž": "z",
         "ß": "ss", "æ": "ae", "œ": "oe"}
FOLD = str.maketrans({c: to for chars, to in FOLDS.items() for c in chars})


def first(value):
    return value[0] if isinstance(value, list) and value else value


def year_of(item: dict) -> int | None:
    """The title's "(YYYY)" first, else the field; never a bare number in a title."""
    m = re.search(r"\(\s*(\d{4})\s*\)", item["title"])
    if m:
        return int(m.group(1))
    try:
        return int(str(item.get("year"))[:4]) if item.get("year") else None
    except ValueError:
        return None


def norm(title: str) -> str:
    t = re.sub(r"\([^)]*\)|\[[^\]]*\]", " ", title.lower().translate(FOLD))
    t = re.sub(NOISE, " ", t)
    t = re.sub(r"[^a-z0-9]+", " ", t).strip()
    return " ".join(re.sub(r"^the ", "", t).split())


def plain(item: dict) -> bool:
    """No dub or edition words in the raw title: preferred among duplicates."""
    return not re.search(NOISE, item["title"].lower())


items = json.load(open(sys.argv[1]))["items"]
for it in items:
    for key in ("title", "year", "licenseurl"):
        it[key] = first(it.get(key))
    it["title"] = it["title"] or ""
print("query", len(items))
dated = [i for i in items if year_of(i) is not None]
print("dropped, no year", len(items) - len(dated))
early = [i for i in dated if year_of(i) <= 1963]
print("dropped, effective year after 1963", len(dated) - len(early))
films_items = [i for i in early if "trailer" not in (i["title"] + i["identifier"]).lower()]
print("dropped, trailers", len(early) - len(films_items))
print("kept items", len(films_items))
best: dict = {}
for it in films_items:
    key = (norm(it["title"]), year_of(it))
    rank = (plain(it), it.get("downloads") or 0)
    if key not in best or rank > best[key][0]:
        best[key] = (rank, it)
dupes = Counter((norm(i["title"]), year_of(i)) for i in films_items)
print("films", len(best), "with duplicates", sum(1 for c in dupes.values() if c > 1))
print("by decade", sorted(Counter((y // 10) * 10 for _, y in best).items()))
print("1929 or before", sum(1 for _, y in best if y <= 1929))
for t in ["night of the living dead", "nosferatu", "metropolis", "his girl friday", "plan 9 from outer space", "white zombie", "charade"]:
    print(t, sorted(y for n, y in best if n == t))
