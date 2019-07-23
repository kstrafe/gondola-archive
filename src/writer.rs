use file_rotate::{FileRotate, RotationMode};
use std::io;

pub fn create_rotational_writer(path: &str) -> impl io::Write {
    FileRotate::new(path, RotationMode::Lines(10_000), 3)
}
