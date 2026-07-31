use flate2::read::GzDecoder;
use gugle_rag::logging::{DEFAULT_MAX_LOG_SIZE, RollingLogWriter};
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
};
use tracing_subscriber::fmt::MakeWriter;
use uuid::Uuid;

#[test]
fn startup_archives_previous_latest_log() {
    let directory = temporary_log_directory();
    fs::write(directory.join("latest.log"), b"previous run\n").unwrap();

    let writer = RollingLogWriter::new(&directory).unwrap();
    drop(writer);

    let archives = archive_paths(&directory);
    assert_eq!(archives.len(), 1);
    assert_eq!(directory.join("latest.log").metadata().unwrap().len(), 0);
    assert_eq!(decode_gzip(&archives[0]), "previous run\n");

    remove_directory(directory);
}

#[test]
fn log_size_rolls_before_writing_the_next_event() {
    let directory = temporary_log_directory();
    let writer = RollingLogWriter::new(&directory).unwrap();
    let mut first = writer.make_writer();
    first
        .write_all(&vec![b'a'; DEFAULT_MAX_LOG_SIZE as usize])
        .unwrap();
    first.flush().unwrap();

    let mut second = writer.make_writer();
    second.write_all(b"next event\n").unwrap();
    second.flush().unwrap();
    drop(second);
    drop(first);
    drop(writer);

    let archives = archive_paths(&directory);
    assert_eq!(archives.len(), 1);
    assert_eq!(
        decode_gzip(&archives[0]).len(),
        DEFAULT_MAX_LOG_SIZE as usize
    );
    assert_eq!(
        fs::read_to_string(directory.join("latest.log")).unwrap(),
        "next event\n"
    );

    remove_directory(directory);
}

fn temporary_log_directory() -> PathBuf {
    let directory = std::env::temp_dir().join(format!("guglerag-logs-{}", Uuid::new_v4()));
    fs::create_dir_all(&directory).unwrap();
    directory
}

fn archive_paths(directory: &PathBuf) -> Vec<PathBuf> {
    let mut paths = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.file_name().unwrap() != "latest.log")
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn decode_gzip(path: &PathBuf) -> String {
    let input = fs::File::open(path).unwrap();
    let mut decoder = GzDecoder::new(input);
    let mut content = String::new();
    decoder.read_to_string(&mut content).unwrap();
    content
}

fn remove_directory(directory: PathBuf) {
    fs::remove_dir_all(directory).unwrap();
}
