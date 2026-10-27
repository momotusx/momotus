//! The Internet Archive's film list (plans/plugins-and-marketplace.md 5.2,
//! 5.3): device-wide, replaced whole, read by normalized title and year.

use crate::store::{Library, LibraryError};
use rusqlite::{OptionalExtension, params};

/// One film of the list: its normalized title, effective year, the item
/// that stands for it, and that item's downloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveFilm {
    pub title: String,
    pub year: u16,
    pub identifier: String,
    pub downloads: u64,
}

impl Library {
    /// Replaces the whole list in one commit.
    pub fn put_archive_index(&self, films: &[ArchiveFilm]) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute("DELETE FROM archive_index", [])?;
            let mut stmt = c.prepare_cached(
                "INSERT OR REPLACE INTO archive_index (title, year, identifier, downloads)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            for f in films {
                let downloads = i64::try_from(f.downloads).unwrap_or(i64::MAX);
                stmt.execute(params![f.title, f.year, f.identifier, downloads])?;
            }
            Ok(())
        })
    }

    /// Deletes the list: no profile on this device wants the source.
    pub fn clear_archive_index(&self) -> Result<(), LibraryError> {
        self.write(|c| c.execute("DELETE FROM archive_index", []).map(drop))
    }

    /// The item for the film `title` (normalized) of `year` or a year
    /// either side: the nearest year first, then the most downloaded.
    pub fn archive_film(&self, title: &str, year: u16) -> Result<Option<String>, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT identifier FROM archive_index
                 WHERE title = ?1 AND year BETWEEN ?2 - 1 AND ?2 + 1
                 ORDER BY abs(year - ?2), downloads DESC LIMIT 1",
                params![title, year],
                |r| r.get(0),
            )
            .optional()
        })
    }

    /// How many films the list holds.
    pub fn archive_films(&self) -> Result<u64, LibraryError> {
        let count: i64 =
            self.read(|c| c.query_row("SELECT COUNT(*) FROM archive_index", [], |r| r.get(0)))?;
        Ok(count.unsigned_abs())
    }
}

#[cfg(test)]
mod tests {
    use super::ArchiveFilm;
    use crate::Library;

    fn film(title: &str, year: u16, identifier: &str, downloads: u64) -> ArchiveFilm {
        ArchiveFilm {
            title: title.into(),
            year,
            identifier: identifier.into(),
            downloads,
        }
    }

    #[test]
    fn a_film_matches_its_year_or_one_either_side_and_the_list_is_replaced_whole() {
        let lib = Library::temporary().unwrap();
        lib.put_archive_index(&[
            film("nosferatu", 1922, "nosferatu-1922", 10),
            film("nosferatu", 1923, "nosferatu-1923", 99),
            film("metropolis", 1927, "metropolis", 5),
        ])
        .unwrap();
        let found = |t: &str, y| lib.archive_film(t, y).unwrap();
        assert_eq!(found("nosferatu", 1922).as_deref(), Some("nosferatu-1922"));
        assert_eq!(found("nosferatu", 1921).as_deref(), Some("nosferatu-1922"));
        assert_eq!(found("nosferatu", 1924).as_deref(), Some("nosferatu-1923"));
        assert_eq!(found("metropolis", 1929), None, "two years off");
        assert_eq!(found("metropoli", 1927), None, "the title is equal");
        lib.put_archive_index(&[film("white zombie", 1932, "wz", 1)])
            .unwrap();
        assert_eq!(lib.archive_films().unwrap(), 1);
        assert_eq!(found("nosferatu", 1922), None);
        lib.clear_archive_index().unwrap();
        assert_eq!(lib.archive_films().unwrap(), 0);
    }
}
