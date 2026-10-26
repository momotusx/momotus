import json, re, sys
from collections import Counter

items = json.load(open(sys.argv[1]))["items"]
first = lambda v: v[0] if isinstance(v, list) and v else v
for it in items:
    for k in ("title", "year", "licenseurl"):
        it[k] = first(it.get(k))
    it["title"] = it["title"] or ""
print("items", len(items))

def year_of(it):
    m = re.search(r"\((\d{4})\)|\b(18[89]\d|19\d\d|20[0-2]\d)\b", it.get("title", ""))
    if m:
        return int(m.group(1) or m.group(2))
    y = it.get("year")
    try:
        return int(str(y)[:4]) if y else None
    except ValueError:
        return None

def norm(title):
    t = re.sub(r"\([^)]*\)|\[[^\]]*\]", " ", title.lower())
    t = re.sub(r"\b(full movie|full hd|hd|restored|english|dubbed|subtitled|colorized|trailer)\b", " ", t)
    t = re.sub(r"[^a-z0-9]+", " ", t)
    t = re.sub(r"^the ", "", t.strip())
    return " ".join(t.split())

pd_mark = lambda u: "publicdomain" in (u or "")
cc_lic = lambda u: "/licenses/" in (u or "")
noyear = [i for i in items if year_of(i) is None]
print("no year", len(noyear))
kept = [i for i in items if year_of(i) is not None]
late_pd = [i for i in kept if year_of(i) > 1963 and not cc_lic(i.get("licenseurl"))]
late_cc = [i for i in kept if year_of(i) > 1963 and cc_lic(i.get("licenseurl"))]
print("after 1963, PD mark only (dropped)", len(late_pd), "after 1963 with a CC licence (kept)", len(late_cc))
kept = [i for i in kept if year_of(i) <= 1963 or cc_lic(i.get("licenseurl"))]
trail = [i for i in kept if "trailer" in i.get("title", "").lower() or "trailer" in i["identifier"].lower()]
print("trailers among kept", len(trail))
kept = [i for i in kept if i not in trail]
groups = Counter((norm(i["title"]), year_of(i)) for i in kept)
print("after no-year, late-PD and trailer drops", len(kept))
print("distinct (title, year) films", len(groups), "of which with duplicates", sum(1 for c in groups.values() if c > 1))
print("by decade", sorted(Counter((year_of(i)//10)*10 for i in kept).items()))
cut = [i for i in kept if year_of(i) <= 1929]
print("films at or before 1929 (US PD by term today)", len({(norm(i['title']), year_of(i)) for i in cut}))
for t in ["night of living dead", "nosferatu", "charade", "his girl friday", "plan 9 from outer space", "white zombie", "metropolis"]:
    print(t, sorted({year_of(i) for i in kept if norm(i["title"]) == t}))
for t in ["night of the living dead", "charade"]:
    print(t, sorted({year_of(i) for i in kept if norm(i["title"]) == t}), [ (i["title"], i.get("year"), i.get("licenseurl")) for i in items if norm(i["title"]).startswith(t)][:4])
print("kept with a CC licence", sum(1 for i in kept if cc_lic(i.get("licenseurl"))), "kept with a PD mark", sum(1 for i in kept if pd_mark(i.get("licenseurl"))))
