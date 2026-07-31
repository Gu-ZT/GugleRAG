use chrono::Local;
use flate2::{Compression, write::GzEncoder};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};
use tracing_subscriber::fmt::MakeWriter;

pub const DEFAULT_MAX_LOG_SIZE: u64 = 500 * 1024;

const LATEST_LOG_NAME: &str = "latest.log";

#[derive(Clone)]
pub struct RollingLogWriter {
    inner: Arc<Mutex<RollingLogState>>,
}

struct RollingLogState {
    log_dir: PathBuf,
    latest_path: PathBuf,
    max_size: u64,
    file: Option<File>,
    size: u64,
}

pub struct RollingLogHandle {
    inner: Arc<Mutex<RollingLogState>>,
}

impl RollingLogWriter {
    pub fn new(log_dir: impl AsRef<Path>) -> io::Result<Self> {
        let log_dir = log_dir.as_ref().to_path_buf();
        fs::create_dir_all(&log_dir)?;
        let latest_path = log_dir.join(LATEST_LOG_NAME);
        archive_existing(&log_dir, &latest_path)?;
        let file = open_latest(&latest_path)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(RollingLogState {
                log_dir,
                latest_path,
                max_size: DEFAULT_MAX_LOG_SIZE,
                file: Some(file),
                size: 0,
            })),
        })
    }

    fn lock_state(&self) -> io::Result<MutexGuard<'_, RollingLogState>> {
        self.inner
            .lock()
            .map_err(|_| io::Error::other("log writer lock poisoned"))
    }
}

impl<'a> MakeWriter<'a> for RollingLogWriter {
    type Writer = RollingLogHandle;

    fn make_writer(&'a self) -> Self::Writer {
        RollingLogHandle {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Write for RollingLogHandle {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let writer = RollingLogWriter {
            inner: Arc::clone(&self.inner),
        };
        let mut state = writer.lock_state()?;
        if state.size > 0
            && state
                .size
                .saturating_add(u64::try_from(buffer.len()).unwrap_or(u64::MAX))
                > state.max_size
        {
            state.rotate()?;
        }
        let file = state
            .file
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "log file is closed"))?;
        let written = file.write(buffer)?;
        state.size = state.size.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        let writer = RollingLogWriter {
            inner: Arc::clone(&self.inner),
        };
        let mut state = writer.lock_state()?;
        state
            .file
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "log file is closed"))?
            .flush()
    }
}

impl RollingLogState {
    fn rotate(&mut self) -> io::Result<()> {
        if let Some(mut file) = self.file.take() {
            file.flush()?;
        }
        archive_existing(&self.log_dir, &self.latest_path)?;
        self.file = Some(open_latest(&self.latest_path)?);
        self.size = 0;
        Ok(())
    }
}

fn open_latest(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
}

fn archive_existing(log_dir: &Path, latest_path: &Path) -> io::Result<bool> {
    let metadata = match fs::metadata(latest_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if !metadata.is_file() || metadata.len() == 0 {
        return Ok(false);
    }

    let archive_path = next_archive_path(log_dir);
    let result = (|| {
        let mut input = File::open(latest_path)?;
        let output = File::create(&archive_path)?;
        let mut encoder = GzEncoder::new(output, Compression::default());
        io::copy(&mut input, &mut encoder)?;
        encoder.finish()?;
        fs::remove_file(latest_path)?;
        Ok::<(), io::Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&archive_path);
    }
    result.map(|_| true)
}

fn next_archive_path(log_dir: &Path) -> PathBuf {
    let timestamp = if cfg!(windows) {
        Local::now().format("%y-%m-%d-%H-%M-%S-%3f").to_string()
    } else {
        Local::now().format("%y-%m-%d-%H:%M:%S:%3f").to_string()
    };
    let base_name = format!("log-{timestamp}.log.gz");
    let first = log_dir.join(&base_name);
    if !first.exists() {
        return first;
    }
    let mut suffix = 1;
    loop {
        let candidate = log_dir.join(format!("log-{timestamp}-{suffix}.log.gz"));
        if !candidate.exists() {
            return candidate;
        }
        suffix += 1;
    }
}
