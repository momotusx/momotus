//! Shared loader for the harvested addon corpus.
#![allow(dead_code)]

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize, Clone)]
pub struct Record {
    pub id: String,
    pub name: String,
    pub text: String,
    pub file: Option<String>,
    pub size: Option<i64>,
}

pub struct Corpus {
    pub addon: String,
    pub records: Vec<Record>,
}

pub fn load() -> Vec<Corpus> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/harvested");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .expect("harvested dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let addon = p.file_stem().unwrap().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&p).expect("read corpus");
            let records = text
                .lines()
                .map(|l| serde_json::from_str(l).expect("corpus line"))
                .collect();
            Corpus { addon, records }
        })
        .collect()
}

/// Release-name candidates: the filename hint and the first text line without its emoji prefix.
pub fn release_strings(corpus: &[Corpus]) -> Vec<String> {
    let mut out = Vec::new();
    for c in corpus {
        for r in &c.records {
            out.extend(r.file.clone());
            let first = r.text.lines().next().unwrap_or("");
            out.push(
                first
                    .trim_start_matches(|ch: char| !ch.is_alphanumeric() && ch != '[' && ch != '(')
                    .to_string(),
            );
        }
    }
    out
}
