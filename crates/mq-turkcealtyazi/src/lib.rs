//! TurkceAltyazi.org read on the device (plans/m13-turkcealtyazi.md). This
//! crate holds the parts that need no network: the title page that lists
//! every subtitle of a title. Every subtitle row on a page must parse; a
//! page where none does, or where the list is gone, means the site changed,
//! never "nothing found".

mod page;

pub use page::{Changed, Kind, Lang, Row, TitlePage, title_page};
