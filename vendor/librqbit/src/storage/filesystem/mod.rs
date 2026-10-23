mod chunks;
mod fs;
mod opened_file;
mod punch;
mod sparse;

pub use chunks::{Chunked, allocated, is_chunked};
pub use fs::{FilesystemStorage, FilesystemStorageFactory};
pub use opened_file::OurFileExt;
pub use punch::punch_supported;
