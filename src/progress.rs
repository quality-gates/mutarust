use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::execution::MutationState;

const TICK_INTERVAL: Duration = Duration::from_millis(100);

/// Thread-safe per-state counts for a running mutation test run.
#[derive(Default)]
pub(crate) struct ProgressCounters {
    killed: AtomicUsize,
    escaped: AtomicUsize,
    errored: AtomicUsize,
    not_covered: AtomicUsize,
    skipped: AtomicUsize,
}

impl ProgressCounters {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Records one finished mutant's state.
    pub(crate) fn record(&self, state: MutationState) {
        let counter = match state {
            MutationState::Generated => return,
            MutationState::Killed => &self.killed,
            MutationState::Escaped => &self.escaped,
            MutationState::Errored => &self.errored,
            MutationState::NotCovered => &self.not_covered,
            MutationState::Skipped => &self.skipped,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            killed: self.killed.load(Ordering::Relaxed),
            escaped: self.escaped.load(Ordering::Relaxed),
            errored: self.errored.load(Ordering::Relaxed),
            not_covered: self.not_covered.load(Ordering::Relaxed),
            skipped: self.skipped.load(Ordering::Relaxed),
        }
    }
}

struct ProgressSnapshot {
    killed: usize,
    escaped: usize,
    errored: usize,
    not_covered: usize,
    skipped: usize,
}

impl ProgressSnapshot {
    fn total(&self) -> usize {
        self.killed + self.escaped + self.errored + self.not_covered + self.skipped
    }
}

/// A live terminal progress line for a mutation run.
///
/// Starts a background thread that redraws a `\r`-prefixed status line on
/// standard error every 100 milliseconds. Dropping the monitor stops the
/// thread and clears the line so later output starts on a clean row.
pub(crate) struct ProgressMonitor {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ProgressMonitor {
    pub(crate) fn start(counters: Arc<ProgressCounters>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let handle = {
            let stop = Arc::clone(&stop);
            thread::spawn(move || run_ticker(&counters, &stop))
        };
        Self {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for ProgressMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn run_ticker(counters: &ProgressCounters, stop: &AtomicBool) {
    let mut stderr = io::stderr();
    while !stop.load(Ordering::SeqCst) {
        thread::sleep(TICK_INTERVAL);
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let _ = write_progress_line(&mut stderr, counters);
    }
    let _ = write!(stderr, "\r\x1b[K");
}

fn write_progress_line(out: &mut impl Write, counters: &ProgressCounters) -> io::Result<()> {
    let snapshot = counters.snapshot();
    write!(
        out,
        "\rProcessed {} mutant(s) ({} killed, {} escaped, {} skipped, {} not covered, {} errored)",
        snapshot.total(),
        snapshot.killed,
        snapshot.escaped,
        snapshot.skipped,
        snapshot.not_covered,
        snapshot.errored
    )
}

#[cfg(test)]
mod tests {
    use super::{ProgressCounters, write_progress_line};
    use crate::execution::MutationState;

    #[test]
    fn write_progress_line_reports_every_state_count() {
        let counters = ProgressCounters::new();
        counters.record(MutationState::Killed);
        counters.record(MutationState::Escaped);
        counters.record(MutationState::Skipped);
        counters.record(MutationState::NotCovered);
        counters.record(MutationState::Errored);
        counters.record(MutationState::Generated);

        let mut output = Vec::new();
        write_progress_line(&mut output, &counters).expect("progress line writes");

        assert_eq!(
            String::from_utf8(output).expect("progress line is UTF-8"),
            "\rProcessed 5 mutant(s) (1 killed, 1 escaped, 1 skipped, 1 not covered, 1 errored)"
        );
    }
}
