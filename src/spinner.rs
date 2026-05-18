//! Liveness spinner for long-running iterations.
//!
//! The runner prints a captured agent response per iteration, then
//! drives the next agent for tens of seconds with nothing on stdout.
//! Without a spinner the user can't distinguish "still working" from
//! "hung". This module owns a small background tokio task that paints
//! a braille spinner + status label + elapsed seconds onto stderr,
//! ~10 frames/sec, and clears the line on stop.
//!
//! Off automatically when stderr is not a TTY, when NO_COLOR is set,
//! or when RALPHTERM_NO_SPINNER=1.

use std::io::{IsTerminal, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::watch;
use tokio::task::JoinHandle;

const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Background spinner. Drop the inner Arc and the spinner clears its
/// line and exits on its own; you can also call `stop()` explicitly to
/// await the painter task.
pub struct Spinner {
    label_tx: watch::Sender<String>,
    stop_tx: watch::Sender<bool>,
    handle: Option<JoinHandle<()>>,
}

impl Spinner {
    /// Start the spinner. Returns `None` when stderr isn't a TTY or the
    /// user has opted out via `NO_COLOR` / `RALPHTERM_NO_SPINNER`. In
    /// those cases the runner just prints status updates inline via
    /// the regular path.
    pub fn start(initial_label: impl Into<String>) -> Option<Arc<Self>> {
        if !std::io::stderr().is_terminal() {
            return None;
        }
        if std::env::var_os("NO_COLOR").is_some() {
            return None;
        }
        if std::env::var_os("RALPHTERM_NO_SPINNER").is_some() {
            return None;
        }

        let initial = initial_label.into();
        let (label_tx, label_rx) = watch::channel(initial);
        let (stop_tx, stop_rx) = watch::channel(false);
        let started = Instant::now();

        let handle = tokio::spawn(paint_loop(label_rx, stop_rx, started));

        Some(Arc::new(Self {
            label_tx,
            stop_tx,
            handle: Some(handle),
        }))
    }

    /// Update the right-hand text shown next to the spinner.
    pub fn set_label(&self, label: impl Into<String>) {
        let _ = self.label_tx.send(label.into());
    }

    /// Stop the spinner and clear its line. Safe to call multiple times.
    pub fn stop(self: &Arc<Self>) {
        let _ = self.stop_tx.send(true);
        // We can't take `self.handle` from behind &Arc, so the painter
        // detects stop_tx and exits on its own; the JoinHandle drains
        // when the Arc is dropped.
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(true);
        // Best effort line clear in case the painter hadn't ticked
        // between stop and drop.
        let mut stderr = std::io::stderr().lock();
        let _ = write!(stderr, "\r\x1b[2K");
        let _ = stderr.flush();
        if let Some(h) = self.handle.take() {
            h.abort();
        }
    }
}

async fn paint_loop(
    mut label_rx: watch::Receiver<String>,
    mut stop_rx: watch::Receiver<bool>,
    started: Instant,
) {
    let mut frame_idx: usize = 0;
    let mut tick = tokio::time::interval(Duration::from_millis(120));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() { break; }
            }
            _ = label_rx.changed() => {}
            _ = tick.tick() => {}
        }
        let label = label_rx.borrow().clone();
        let elapsed = started.elapsed().as_secs();
        let frame = FRAMES[frame_idx % FRAMES.len()];
        frame_idx = frame_idx.wrapping_add(1);
        // ANSI: \r → carriage return to column 0;
        //       \x1b[2K → clear the whole line.
        // Dim cyan for the spinner glyph, dim for the elapsed seconds.
        let painted = format!("\r\x1b[2K\x1b[36m{frame}\x1b[0m {label} \x1b[2m({elapsed}s)\x1b[0m");
        let mut stderr = std::io::stderr().lock();
        let _ = stderr.write_all(painted.as_bytes());
        let _ = stderr.flush();
    }
    // Final clear so the next stdout line lands on a blank row.
    let mut stderr = std::io::stderr().lock();
    let _ = write!(stderr, "\r\x1b[2K");
    let _ = stderr.flush();
}

/// Translate a `crate::agent_driver::DriverEvent.kind` into a
/// human-friendly spinner label. Returns `None` for kinds we don't
/// want to surface (so the previous label stays).
pub fn label_for_event(kind: &str) -> Option<&'static str> {
    match kind {
        "agent_started" => Some("agent spawned, waiting for REPL ready"),
        "bypass_permissions_dialog_seen" => Some("dismissing bypass-permissions dialog"),
        "agent_prompt_pasted" => Some("prompt delivered, waiting for response"),
        "agent_prompt_submitted" => Some("waiting for agent response"),
        "agent_output_file_complete" => Some("captured response, reaping agent"),
        "agent_completed" => Some("iteration complete"),
        "agent_timed_out" => Some("idle timeout"),
        "agent_cancelled" => Some("cancelled"),
        "agent_crashed_before_done" => Some("agent crashed before END marker"),
        "agent_exited_without_file" => Some("agent exited without writing output file"),
        _ => None,
    }
}
