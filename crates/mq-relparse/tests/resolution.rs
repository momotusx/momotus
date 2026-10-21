//! The resolution of real release names, from 240p to 4320p, and names
//! that carry two numbers (a year that looks like a height, a frame size,
//! a frame rate).

use mq_relparse::{Resolution as R, parse};

#[test]
fn real_release_names_read_their_own_resolution() {
    let cases = [
        ("Big.Buck.Bunny.2008.240p.WEB.VP9-OPENFILM", R::R240p),
        ("Big Buck Bunny (2008) [426x240] VP9", R::R240p),
        (
            "Night.of.the.Living.Dead.1968.360p.WEB.h264-ARCHIVE",
            R::R360p,
        ),
        ("Sintel.2010.640x360.WEB.VP9", R::R360p),
        ("The.Office.S01E01.480p.WEB.x264-mSD", R::R480p),
        ("Sintel.2010.854x480.x264", R::R480p),
        ("Doctor.Who.1963.S01E01.576p.DVDRip.x264", R::R576p),
        ("Movie.1999.720x576.PAL.DVD", R::R576p),
        ("Severance.S02E10.720p.ATVP.WEB-DL.x264-MIKE", R::R720p),
        ("Sintel.2010.1280x720.mkv", R::R720p),
        ("Inception.2010.1080p.BluRay.x264-YIFY", R::R1080p),
        ("Movie.2020.1080i.HDTV.MPEG2", R::R1080p),
        ("Movie.2019.1920x1080.WEB-DL.H264", R::R1080p),
        ("Big.Buck.Bunny.2008.1080p.60fps.x264", R::R1080p),
        ("Dune.Part.Two.2024.1440p.WEB-DL.x265", R::R1440p),
        ("Dune.Part.Two.2024.2160p.UHD.BluRay.x265-GRP", R::R2160p),
        ("Avatar.2009.4K.HDR.BluRay.x265", R::R2160p),
        ("Oppenheimer.2023.UHD.BluRay.HDR.x265-GRP", R::R2160p),
        ("Oppenheimer.2023.1080p.UHD.BluRay.x265-GRP", R::R1080p),
        ("Tears.of.Steel.2012.3840x2160.WEB", R::R2160p),
        ("Movie.2023.4320p.8K.WEB", R::R4320p),
        // A year that looks like a height is no height.
        ("Blade.Runner.2049.2017.1080p.BluRay.x264", R::R1080p),
        ("1917.2019.2160p.WEB-DL.x265", R::R2160p),
        ("2012.2009.720p.BluRay.x264", R::R720p),
        ("Movie.1080.2019.WEB", R::Unknown),
        // A restored master is how 1080p encodes say 4K.
        (
            "The.Shawshank.Redemption.1994.RM4K.1080p.BluRay.x265",
            R::R1080p,
        ),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(name, want)| {
            let got = parse(name).resolution;
            (got != *want).then(|| format!("{name}: {got:?}, want {want:?}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
