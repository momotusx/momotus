//! TurkceAltyazi.org read on the device (plans/m13-turkcealtyazi.md): the
//! title page that lists every subtitle of a title, the archive a subtitle
//! downloads as, the ranking against the playing release, and the site
//! client that fetches them politely and says what went wrong.
//! Every subtitle row on a page must parse; a page where too many do not,
//! or where the list is gone, means the site changed, never "nothing found".

mod archive;
mod form;
mod page;
mod rank;
mod site;

pub use archive::{ArchiveFault, MAX_ARCHIVE, MAX_ENTRY, Want, pick};
pub use form::{Form, download_form};
pub use page::{Changed, Kind, Lang, Row, TitlePage, title_page};
pub use rank::{MAX_TRACKS, Playing, Target, Track, tracks};
pub use site::{Fault, Fetched, Limits, Listing, ORIGIN, Problem, Site};
