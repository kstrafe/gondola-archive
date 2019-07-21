use pipe_logger_lib::*;
use std::io::Write;

pub fn create_rotational_writer() -> impl Write {
    let mut builder = PipeLoggerBuilder::new("files/logs/log");
    builder.set_tee(Some(Tee::Stdout));
    builder.set_rotate(Some(RotateMethod::FileSize(3_000_000)));
    builder.set_count(Some(3));
    builder.set_compress(false);
    builder.build().expect("Unable to create rotational writer")
}
