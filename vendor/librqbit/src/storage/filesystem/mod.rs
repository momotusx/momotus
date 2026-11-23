mod chunks;
mod fs;
mod opened_file;
mod punch;
mod sparse;

#[cfg(windows)]
pub use chunks::allocated_ranges;
pub use chunks::{Chunked, allocated, is_chunked};
pub use fs::{FilesystemStorage, FilesystemStorageFactory};
pub use opened_file::OurFileExt;
pub use punch::{punch_probe, punch_supported};
