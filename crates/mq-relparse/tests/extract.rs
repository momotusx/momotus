//! Field coverage of the addon extractors over the harvested corpus.

mod common;

use mq_relparse::parse_addon_stream;

struct Coverage {
    total: usize,
    seeders: usize,
    size: usize,
    site: usize,
    resolution: usize,
    title: usize,
}

fn coverage(addon: &str) -> Coverage {
    let corpus = common::load();
    let c = corpus.iter().find(|c| c.addon == addon).expect("corpus");
    let mut cov = Coverage {
        total: 0,
        seeders: 0,
        size: 0,
        site: 0,
        resolution: 0,
        title: 0,
    };
    for r in &c.records {
        let f = parse_addon_stream(&r.name, &r.text, r.file.as_deref());
        cov.total += 1;
        cov.seeders += usize::from(f.seeders.is_some());
        cov.size += usize::from(f.size_bytes.is_some());
        cov.site += usize::from(f.source_site.is_some());
        cov.resolution += usize::from(f.release.resolution.height() > 0);
        cov.title += usize::from(!f.release.title.is_empty());
    }
    println!(
        "{addon}: n={} seeders {} size {} site {} resolution {} title {}",
        cov.total, cov.seeders, cov.size, cov.site, cov.resolution, cov.title
    );
    cov
}

fn at_least(count: usize, total: usize, percent: usize) -> bool {
    count * 100 >= total * percent
}

#[test]
fn torrentio_style_lines_are_fully_extracted() {
    for addon in ["torrentio", "torrentsdb"] {
        let c = coverage(addon);
        assert!(at_least(c.seeders, c.total, 99), "{addon} seeders");
        assert!(at_least(c.size, c.total, 99), "{addon} size");
        assert!(at_least(c.site, c.total, 99), "{addon} site");
        assert!(at_least(c.title, c.total, 98), "{addon} title");
    }
}

#[test]
fn comet_lines_are_extracted() {
    let c = coverage("comet");
    assert!(at_least(c.size, c.total, 99), "size");
    assert!(at_least(c.site, c.total, 99), "site");
    assert!(at_least(c.title, c.total, 98), "title");
    // Comet prints seeders only for torrents it could count, so coverage is partial by design.
    assert!(c.seeders > 0);
}
