//! TurkceAltyazi.org read on the device (plans/m13-turkcealtyazi.md). This
//! crate holds the parts that need no network: the title page that lists
//! every subtitle of a title, and the archive a subtitle downloads as.
//! Every subtitle row on a page must parse; a page where too many do not,
//! or where the list is gone, means the site changed, never "nothing found".

mod archive;
mod page;

pub use archive::{ArchiveFault, MAX_ARCHIVE, MAX_ENTRY, Want, pick};
pub use page::{Changed, Kind, Lang, Row, TitlePage, title_page};
