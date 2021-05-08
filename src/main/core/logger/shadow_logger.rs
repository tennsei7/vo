use crate::core::support::simulation_time::SimulationTime;
use crate::core::worker::Worker;
use crossbeam::queue::SegQueue;
use log::*;
use log::{Level, Log, Metadata, Record, SetLoggerError};
use log_bindings as c_log;
use once_cell::sync::Lazy;
use std::cell::RefCell;
use std::convert::TryFrom;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, RwLock};
use std::time::Duration;

/// Trigger an asynchronous flush when this many lines are queued.
const ASYNC_FLUSH_QD_LINES_THRESHOLD: usize = 100_000;

/// Performs a *synchronous* flush when this many lines are queued.  i.e. if
/// after reaching the `ASYNC_FLUSH_QD_LINES_THRESHOLD`, log lines are still
/// coming in faster than they can actually be flushed, when we reach this limit
/// we'll pause and let it finish flushing rather than letting the queue
/// continue growing.
const SYNC_FLUSH_QD_LINES_THRESHOLD: usize = 10 * ASYNC_FLUSH_QD_LINES_THRESHOLD;

/// Logging thread flushes at least this often.
const MIN_FLUSH_FREQUENCY: Duration = Duration::from_secs(10);

static SHADOW_LOGGER: Lazy<ShadowLogger> = Lazy::new(|| ShadowLogger::new());

/// Helper for formatting times.
#[derive(Debug, Eq, PartialEq)]
struct TimeParts {
    hours: u32,
    mins: u32,
    secs: u64,
    nanos: u64,
}

impl TimeParts {
    fn from_nanos(total_nanos: u128) -> Self {
        // Total number of integer seconds.
        let whole_secs = u64::try_from(total_nanos / 1_000_000_000).unwrap();
        // Total number of integer minutes.
        let whole_mins = u32::try_from(whole_secs / 60).unwrap();
        // Total number of integer hours, which is also the hours part.
        let whole_hours = whole_mins / 60;

        // Integer minutes, after whole hours are subtracted out.
        let mins_part = whole_mins - whole_hours * 60;
        // Integers secs, after integer minutes are subtracted out.
        let secs_part = whole_secs - u64::from(whole_mins) * 60;
        // Nanos, after integer secs are subtracted out.
        let nanos_part =
            u64::try_from(total_nanos - u128::from(whole_secs) * 1_000_000_000).unwrap();

        Self {
            hours: whole_hours,
            mins: mins_part,
            secs: secs_part,
            nanos: nanos_part,
        }
    }
}

#[cfg(test)]
#[test]
fn test_time_parts() {
    assert_eq!(
        TimeParts::from_nanos(
            (Duration::from_nanos(1) + Duration::from_secs(3600 + 60 + 1)).as_nanos()
        ),
        TimeParts {
            hours: 1,
            mins: 1,
            secs: 1,
            nanos: 1
        }
    );
}

/// Initialize the Shadow logger.
pub fn init() -> Result<(), SetLoggerError> {
    log::set_logger(&*SHADOW_LOGGER)?;

    // Start the thread that will receive log records and flush them to output.
    std::thread::Builder::new()
        .name("shadow-logger".to_string())
        .spawn(move || SHADOW_LOGGER.logger_thread_fn())
        .unwrap();

    // Arrange to flush the logger on panic.
    let default_panic_handler = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // Attempt to flush the logger. We want to avoid a recursive panic, so
        // we flush the queue on the current thread instead of trying to send
        // a command to the logger thread (because our thread-local sender
        // may have already been destructed, and because the logger thread
        // itself may be in a bad state), and ignore errors.
        SHADOW_LOGGER.flush_records(None).ok();
        default_panic_handler(panic_info);
    }));

    Ok(())
}

/// A logger specialized for Shadow. It attaches simulation context to log
/// entries (e.g. sim time, running process, etc.). It's also designed for
/// high performance to accomodate heavy logging from multiple threads.
pub struct ShadowLogger {
    // Channel used to send commands to the logger's thread.
    //
    // The Sender half of a channel isn't Sync, so we must protect it with a
    // Mutex to make ShadowLogger be Sync. This is only accessed once per
    // thread, though, to clone into the thread-local SENDER.
    command_sender: Mutex<Sender<LoggerCommand>>,

    // Like the sender, needs a Mutex for ShadowLogger to be Sync.
    // The Mutex is only locked once though by the logger thread, which keeps
    // it locked for as long as it's running.
    command_receiver: Mutex<Receiver<LoggerCommand>>,

    // A lock-free queue for individual log records. We don't put the records
    // themselves in the `command_sender`, because `Sender` doesn't support
    // getting the queue length. Conversely we don't put commands in this queue
    // because it doesn't support blocking operations.
    records: SegQueue<ShadowLogRecord>,

    // When false, sends a (still-asynchronous) flush command to the logger
    // thread every time a record is pushed into `records`.
    buffering_enabled: RwLock<bool>,
}

thread_local!(static SENDER: RefCell<Option<Sender<LoggerCommand>>> = RefCell::new(None));

impl ShadowLogger {
    fn new() -> ShadowLogger {
        let (sender, receiver) = std::sync::mpsc::channel();
        let logger = ShadowLogger {
            records: SegQueue::new(),
            command_sender: Mutex::new(sender),
            command_receiver: Mutex::new(receiver),
            buffering_enabled: RwLock::new(false),
        };
        logger
    }

    // Function executed by the logger's helper thread, onto which we offload as
    // much work as we can.
    fn logger_thread_fn(&self) {
        let command_receiver = self.command_receiver.lock().unwrap();

        loop {
            use std::sync::mpsc::RecvTimeoutError;
            match command_receiver.recv_timeout(MIN_FLUSH_FREQUENCY) {
                Ok(LoggerCommand::Flush(done_sender)) => self.flush_records(done_sender).unwrap(),
                Err(RecvTimeoutError::Timeout) => {
                    // Flush
                    self.flush_records(None).unwrap();
                }
                Err(e) => panic!("Unexpected error {}", e),
            }
        }
    }

    // Function called by the logger's helper thread to flush the contents of
    // self.records. If `done_sender` is provided, it's notified after the flush
    // has completed.
    fn flush_records(&self, done_sender: Option<Sender<()>>) -> std::io::Result<()> {
        use std::io::Write;

        // Only flush records that are already in the queue, not ones that
        // arrive while we're flushing. Otherwise callers who perform a
        // synchronous flush (whether this flush operation or another one that
        // arrives while we're flushing) will be left waiting longer than
        // necessary. Also keeps us from holding the stdout lock indefinitely.
        let mut toflush = self.records.len();

        let stdout_unlocked = std::io::stdout();
        let stdout_locked = stdout_unlocked.lock();
        let mut stdout = std::io::BufWriter::new(stdout_locked);
        while toflush > 0 {
            let record = self.records.pop().unwrap();
            toflush -= 1;
            {
                let parts = TimeParts::from_nanos(record.wall_time.as_nanos());
                write!(
                    stdout,
                    "{:02}:{:02}:{:02}.{:06}",
                    parts.hours,
                    parts.mins,
                    parts.secs,
                    parts.nanos / 1000
                )?;
            }
            if let Some(id) = record.thread_id {
                write!(stdout, " [thread-{}]", id)?;
            } else {
                write!(stdout, " [n/a]")?;
            }
            if let Some(sim_time) = record.sim_time {
                let parts = TimeParts::from_nanos(sim_time.as_nanos());
                write!(
                    stdout,
                    " {:02}:{:02}:{:02}.{:09}",
                    parts.hours, parts.mins, parts.secs, parts.nanos
                )?;
            } else {
                write!(stdout, " n/a")?;
            }
            write!(
                stdout,
                " [{level}] [{host}] [{file}:{line}] [{module}] {msg}\n",
                level = record.level,
                host = record
                    .host_name
                    .as_ref()
                    .map(|s| s.as_str())
                    .unwrap_or("n/a"),
                file = record
                    .file
                    .map(|f| if let Some(sep_pos) = f.rfind('/') {
                        &f[(sep_pos + 1)..]
                    } else {
                        f
                    })
                    .unwrap_or("n/a"),
                line = record
                    .line
                    .map(|l| format!("{}", l))
                    .as_ref()
                    .map(|s| s.as_str())
                    .unwrap_or("n/a"),
                module = record.module_path.unwrap_or("n/a"),
                msg = record.message
            )?;
        }
        if let Some(done_sender) = done_sender {
            done_sender
                .send(())
                .unwrap_or_else(|e| warn!("Couldn't notify calling thread: {:?}", e));
        }
        Ok(())
    }

    /// When disabled, the logger thread is notified to write each record as
    /// soon as it's created.  The calling thread still isn't blocked on the
    /// record actually being written, though.
    pub fn set_buffering_enabled(&self, buffering_enabled: bool) {
        let mut writer = self.buffering_enabled.write().unwrap();
        *writer = buffering_enabled;
    }

    // Send a flush command to the logger thread.
    fn flush_impl(&self, notify_done: Option<Sender<()>>) {
        self.send_command(LoggerCommand::Flush(notify_done))
    }

    // Send a flush command to the logger thread and block until it's completed.
    fn flush_sync(&self) {
        let (done_sender, done_receiver) = std::sync::mpsc::channel();
        self.flush_impl(Some(done_sender));
        done_receiver.recv().unwrap();
    }

    // Send a flush command to the logger thread.
    fn flush_async(&self) {
        self.flush_impl(None);
    }

    // Send a command to the logger thread.
    fn send_command(&self, cmd: LoggerCommand) {
        SENDER.with(|sender| {
            if sender.borrow().is_none() {
                let lock = self.command_sender.lock().unwrap();
                *sender.borrow_mut() = Some(lock.clone());
            }
            sender.borrow().as_ref().unwrap().send(cmd).unwrap();
        });
    }
}

impl Log for ShadowLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let message = std::fmt::format(*record.args());

        let host_name = Worker::with_active_host(|host| {
            let name = host.name();
            let ip = host.default_ip();
            format!("{}~{}", name, ip)
        });

        self.records.push(ShadowLogRecord {
            level: record.level(),
            file: record.file_static(),
            module_path: record.module_path_static(),
            line: record.line(),
            message,
            wall_time: Duration::from_micros(unsafe {
                u64::try_from(c_log::logger_elapsed_micros()).unwrap()
            }),

            sim_time: Worker::current_time(),
            thread_id: Worker::thread_id(),
            host_name,
        });

        if record.level() == Level::Error || self.records.len() > SYNC_FLUSH_QD_LINES_THRESHOLD {
            // Unlike in Shadow's C code, we don't abort the program on Error
            // logs. In Rust the same purpose is filled with `panic` and
            // `unwrap`. C callers will still exit or abort via the support/logger wrapper.
            //
            // Flush *synchronously*, since we're likely about to crash one way or another.
            self.flush_sync();
        } else if self.records.len() > ASYNC_FLUSH_QD_LINES_THRESHOLD
            || !*self.buffering_enabled.read().unwrap()
        {
            self.flush_async();
        }
    }

    fn flush(&self) {
        self.flush_sync();
    }
}

struct ShadowLogRecord {
    level: Level,
    file: Option<&'static str>,
    module_path: Option<&'static str>,
    line: Option<u32>,
    message: String,
    wall_time: Duration,

    sim_time: Option<SimulationTime>,
    thread_id: Option<i32>,
    host_name: Option<String>,
}

enum LoggerCommand {
    // Flush; takes an optional one-shot channel to notify that the flush has completed.
    Flush(Option<Sender<()>>),
}

mod export {
    use super::*;

    /// Creates a ShadowLogger and installs it as the default logger for Rust's
    /// `log` crate. The returned pointer is never deallocated, since loggers
    /// registered with the `log` crate are required to live for the life of the
    /// program.
    #[no_mangle]
    pub unsafe extern "C" fn shadow_logger_init() -> () {
        init().unwrap()
    }

    /// When disabled, the logger thread is notified to write each record as
    /// soon as it's created.  The calling thread still isn't blocked on the
    /// record actually being written, though.
    #[no_mangle]
    pub unsafe extern "C" fn shadow_logger_setEnableBuffering(buffering_enabled: i32) {
        SHADOW_LOGGER.set_buffering_enabled(buffering_enabled != 0)
    }
}
