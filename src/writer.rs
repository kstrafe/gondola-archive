use file_rotate::{compression::Compression, suffix::AppendCount, ContentLimit, FileRotate};
use std::io;

pub fn create_rotational_writer(path: &str) -> impl io::Write {
    FileRotate::new(
        path,
        AppendCount::new(3),
        ContentLimit::Bytes(100_000),
        Compression::None,
        None,
    )
}
