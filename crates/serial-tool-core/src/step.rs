use std::{
    collections::HashSet,
    error, fmt,
    fs::File,
    io::{self, Read},
    path::Path,
    thread,
    time::Duration,
};

use crate::{Error, SerialSession, Transport};

pub const MAX_READ_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StepId(String);

impl StepId {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidStepId> {
        let value = value.into();
        if value.is_empty()
            || value.split('-').any(|part| {
                part.is_empty()
                    || !part
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            })
        {
            return Err(InvalidStepId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StepId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidStepId(pub String);

impl fmt::Display for InvalidStepId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid step ID: {:?}", self.0)
    }
}

impl error::Error for InvalidStepId {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub id: StepId,
    pub kind: StepKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind {
    SendText(String),
    SendBytes(Vec<u8>),
    SendFile(String),
    Wait(Duration),
    Read {
        max_bytes: usize,
    },
    ReadUntil {
        delimiter: Vec<u8>,
        max_bytes: usize,
    },
    Repeat {
        count: usize,
        steps: Vec<Step>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub step_id: StepId,
    pub reason: ValidationReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationReason {
    DuplicateId,
    EmptySend,
    InvalidFilePath,
    InvalidReadLimit,
    EmptyDelimiter,
    DelimiterExceedsReadLimit,
    ZeroRepeatCount,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid step {}: {:?}", self.step_id, self.reason)
    }
}

impl error::Error for ValidationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepResult {
    pub step_id: StepId,
    pub outcome: StepOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    SendText { bytes_written: usize },
    SendBytes { bytes_written: usize },
    SendFile { file_name: String, file_bytes: u64, bytes_written: u64 },
    Wait { requested_duration: Duration },
    Read { bytes: Vec<u8> },
    ReadUntil { bytes: Vec<u8> },
    Repeat { completed_iterations: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Tx,
    Rx,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptEntry {
    pub step_id: StepId,
    pub direction: Direction,
    pub bytes: Vec<u8>,
}

/// Logical I/O from completed steps, not an independent physical-wire capture.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Transcript {
    pub entries: Vec<TranscriptEntry>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RunReport {
    /// Completed steps in completion order; repeat children retain their original IDs.
    pub step_results: Vec<StepResult>,
    pub transcript: Transcript,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTransfer {
    pub file_name: String,
    pub file_bytes: Option<u64>,
    pub bytes_written: u64,
}

#[derive(Debug)]
pub struct ExecutionFailure {
    pub step_id: StepId,
    pub error: Error,
    pub report: RunReport,
    pub file_transfer: Option<FileTransfer>,
}

impl fmt::Display for ExecutionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "step {} failed: {}", self.step_id, self.error)
    }
}

impl error::Error for ExecutionFailure {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(&self.error)
    }
}

#[derive(Debug)]
pub enum RunError {
    Validation(ValidationError),
    Execution(ExecutionFailure),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(f),
            Self::Execution(error) => error.fmt(f),
        }
    }
}

impl error::Error for RunError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Validation(error) => Some(error),
            Self::Execution(error) => Some(error),
        }
    }
}

pub struct StepRunner<'a, T: Transport> {
    session: &'a mut SerialSession<T>,
}

impl<'a, T: Transport> StepRunner<'a, T> {
    pub fn new(session: &'a mut SerialSession<T>) -> Self {
        Self { session }
    }

    pub fn run(&mut self, steps: &[Step]) -> Result<RunReport, RunError> {
        validate_steps(steps).map_err(RunError::Validation)?;
        let mut report = RunReport::default();
        if let Err((step_id, error, file_transfer)) = self.execute(steps, &mut report) {
            return Err(RunError::Execution(ExecutionFailure {
                step_id,
                error,
                report,
                file_transfer,
            }));
        }
        Ok(report)
    }

    fn execute(&mut self, steps: &[Step], report: &mut RunReport) -> Result<(), (StepId, Error, Option<FileTransfer>)> {
        for step in steps {
            let outcome = match &step.kind {
                StepKind::SendText(text) => {
                    self.send(step, text.as_bytes(), report)?;
                    StepOutcome::SendText {
                        bytes_written: text.len(),
                    }
                }
                StepKind::SendBytes(bytes) => {
                    self.send(step, bytes, report)?;
                    StepOutcome::SendBytes {
                        bytes_written: bytes.len(),
                    }
                }
                StepKind::SendFile(path) => self
                    .send_file(path)
                    .map_err(|(error, progress)| (step.id.clone(), error, Some(progress)))?,
                StepKind::Wait(duration) => {
                    thread::sleep(*duration);
                    StepOutcome::Wait {
                        requested_duration: *duration,
                    }
                }
                StepKind::Read { max_bytes } => {
                    let mut bytes = vec![0; *max_bytes];
                    let count = self
                        .session
                        .read(&mut bytes)
                        .map_err(|error| (step.id.clone(), error, None))?;
                    bytes.truncate(count);
                    report.transcript.entries.push(TranscriptEntry {
                        step_id: step.id.clone(),
                        direction: Direction::Rx,
                        bytes: bytes.clone(),
                    });
                    StepOutcome::Read { bytes }
                }
                StepKind::ReadUntil {
                    delimiter,
                    max_bytes,
                } => {
                    let bytes = self
                        .session
                        .read_until(delimiter, *max_bytes)
                        .map_err(|error| (step.id.clone(), error, None))?;
                    report.transcript.entries.push(TranscriptEntry {
                        step_id: step.id.clone(),
                        direction: Direction::Rx,
                        bytes: bytes.clone(),
                    });
                    StepOutcome::ReadUntil { bytes }
                }
                StepKind::Repeat { count, steps } => {
                    for _ in 0..*count {
                        self.execute(steps, report)?;
                    }
                    StepOutcome::Repeat {
                        completed_iterations: *count,
                    }
                }
            };
            report.step_results.push(StepResult {
                step_id: step.id.clone(),
                outcome,
            });
        }
        Ok(())
    }

    fn send(
        &mut self,
        step: &Step,
        bytes: &[u8],
        report: &mut RunReport,
    ) -> Result<(), (StepId, Error, Option<FileTransfer>)> {
        self.session
            .write_all(bytes)
            .and_then(|()| self.session.flush())
            .map_err(|error| (step.id.clone(), error, None))?;
        report.transcript.entries.push(TranscriptEntry {
            step_id: step.id.clone(),
            direction: Direction::Tx,
            bytes: bytes.to_vec(),
        });
        Ok(())
    }

    fn send_file(&mut self, path: &str) -> Result<StepOutcome, (Error, FileTransfer)> {
        let mut progress = FileTransfer {
            file_name: Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_owned()),
            file_bytes: None,
            bytes_written: 0,
        };
        let file_error = |source| Error::FileFailed { path: path.into(), source };
        let mut file = File::open(path).map_err(|source| (file_error(source), progress.clone()))?;
        let metadata = file.metadata().map_err(|source| (file_error(source), progress.clone()))?;
        if !metadata.is_file() {
            return Err((file_error(io::Error::new(io::ErrorKind::InvalidInput, "not a regular file")), progress));
        }
        progress.file_bytes = Some(metadata.len());
        if metadata.len() == 0 {
            return Err((file_error(io::Error::new(io::ErrorKind::InvalidData, "file must not be empty")), progress));
        }
        let mut buffer = [0u8; 4096];
        loop {
            let count = match file.read(&mut buffer) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                read => read.map_err(|source| (file_error(source), progress.clone()))?,
            };
            if count == 0 {
                return Ok(StepOutcome::SendFile {
                    file_name: progress.file_name,
                    file_bytes: metadata.len(),
                    bytes_written: progress.bytes_written,
                });
            }
            self.session
                .write_all(&buffer[..count])
                .and_then(|()| self.session.flush())
                .map_err(|error| (error, progress.clone()))?;
            progress.bytes_written += count as u64;
        }
    }
}

pub fn valid_local_file_path(path: &str) -> bool {
    if path.trim().is_empty() || path.contains('\0') {
        return false;
    }
    let candidate = Path::new(path);
    if !candidate.is_absolute() {
        return false;
    }
    #[cfg(windows)]
    if !matches!(candidate.components().next(), Some(std::path::Component::Prefix(prefix))
        if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)))
    {
        return false;
    }
    true
}

pub fn validate_steps(steps: &[Step]) -> Result<(), ValidationError> {
    fn visit<'a>(steps: &'a [Step], seen: &mut HashSet<&'a StepId>) -> Result<(), ValidationError> {
        for step in steps {
            let reason = if !seen.insert(&step.id) {
                Some(ValidationReason::DuplicateId)
            } else {
                match &step.kind {
                    StepKind::SendText(text) if text.is_empty() => {
                        Some(ValidationReason::EmptySend)
                    }
                    StepKind::SendBytes(bytes) if bytes.is_empty() => {
                        Some(ValidationReason::EmptySend)
                    }
                    StepKind::SendFile(path) if !valid_local_file_path(path) => {
                        Some(ValidationReason::InvalidFilePath)
                    }
                    StepKind::Read { max_bytes } | StepKind::ReadUntil { max_bytes, .. }
                        if *max_bytes == 0 || *max_bytes > MAX_READ_BYTES =>
                    {
                        Some(ValidationReason::InvalidReadLimit)
                    }
                    StepKind::ReadUntil { delimiter, .. } if delimiter.is_empty() => {
                        Some(ValidationReason::EmptyDelimiter)
                    }
                    StepKind::ReadUntil {
                        delimiter,
                        max_bytes,
                    } if delimiter.len() > *max_bytes => {
                        Some(ValidationReason::DelimiterExceedsReadLimit)
                    }
                    StepKind::Repeat { count: 0, .. } => Some(ValidationReason::ZeroRepeatCount),
                    _ => None,
                }
            };
            if let Some(reason) = reason {
                return Err(ValidationError {
                    step_id: step.id.clone(),
                    reason,
                });
            }
            if let StepKind::Repeat { steps, .. } = &step.kind {
                visit(steps, seen)?;
            }
        }
        Ok(())
    }

    visit(steps, &mut HashSet::new())
}
