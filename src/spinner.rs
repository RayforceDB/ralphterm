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

/// Best-effort terminal width. Falls back to 80 when we can't get one.
fn terminal_columns() -> usize {
    if let Ok(val) = std::env::var("COLUMNS") {
        if let Ok(n) = val.parse::<usize>() {
            if n > 0 {
                return n;
            }
        }
    }
    80
}

/// Truncate `text` so the painted line stays on ONE physical row.
/// Without this, a long label wraps and the painter's `\r\x1b[2K`
/// (clear current line only) leaks a new row every frame.
fn fit_to_width(text: &str, columns: usize) -> String {
    if text.chars().count() <= columns {
        return text.to_string();
    }
    if columns < 4 {
        return text.chars().take(columns).collect();
    }
    let take = columns.saturating_sub(1);
    let truncated: String = text.chars().take(take).collect();
    format!("{truncated}…")
}

const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Background spinner. Drop the inner Arc and the spinner clears its
/// line and exits on its own; you can also call `stop()` explicitly to
/// await the painter task.
pub struct Spinner {
    label_tx: watch::Sender<String>,
    activity_tx: watch::Sender<Instant>,
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
        let started = Instant::now();
        let (label_tx, label_rx) = watch::channel(initial);
        let (activity_tx, activity_rx) = watch::channel(started);
        let (stop_tx, stop_rx) = watch::channel(false);

        let handle = tokio::spawn(paint_loop(label_rx, activity_rx, stop_rx, started));

        Some(Arc::new(Self {
            label_tx,
            activity_tx,
            stop_tx,
            handle: Some(handle),
        }))
    }

    /// Update the right-hand text shown next to the spinner. Also
    /// counts as activity (resets the "idle Ns" suffix).
    pub fn set_label(&self, label: impl Into<String>) {
        let _ = self.label_tx.send(label.into());
        let _ = self.activity_tx.send(Instant::now());
    }

    /// Mark activity without changing the label. Use this when an
    /// event arrived but doesn't need a new label — typically a data
    /// heartbeat from a long-running child. The painter resets its
    /// "idle Ns" counter on the next frame.
    pub fn bump_activity(&self) {
        let _ = self.activity_tx.send(Instant::now());
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
    mut activity_rx: watch::Receiver<Instant>,
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
            _ = activity_rx.changed() => {}
            _ = tick.tick() => {}
        }
        let label = label_rx.borrow().clone();
        let last_activity = *activity_rx.borrow();
        let elapsed = started.elapsed().as_secs();
        let idle = last_activity.elapsed().as_secs();
        let frame = FRAMES[frame_idx % FRAMES.len()];
        frame_idx = frame_idx.wrapping_add(1);
        // ANSI: \r → carriage return to column 0;
        //       \x1b[2K → clear the whole line.
        // Dim cyan glyph, dim trailing timestamps. When `idle` climbs
        // significantly past zero with no label change, that's the
        // signal: the agent is silent, suspect rate-limit or hang.
        // Past 30 s of idle we add a heads-up about claude's silent
        // tool-use / extended-thinking phase so the user can tell
        // "still working" from "actually stuck".
        let timing = if idle >= 30 {
            format!(
                "({elapsed}s elapsed, idle {idle}s — agent may be in silent tool-use / extended-thinking)"
            )
        } else if idle >= 3 {
            format!("({elapsed}s elapsed, idle {idle}s)")
        } else {
            format!("({elapsed}s)")
        };
        // Truncate label + timing to fit the terminal width so a long
        // label can't wrap. `\r\x1b[2K` only clears the line the
        // cursor is on, so wrapped output leaks a new physical row
        // every paint frame (user reported this on v0.4.6).
        // 2-char budget for the frame glyph + space; rest for label
        // and the dim timing suffix.
        let cols = terminal_columns();
        let body = format!("{label} {timing}");
        let body_budget = cols.saturating_sub(3); // glyph + space + safety
        let body_fitted = fit_to_width(&body, body_budget);
        let painted = format!("\r\x1b[2K\x1b[36m{frame}\x1b[0m \x1b[2m{body_fitted}\x1b[0m");
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
        // agent_data fires every ~2s while the child is streaming
        // bytes; the runner's event sink should call bump_activity()
        // on it rather than overwriting the label.
        "agent_data" => None,
        // agent_writing_output fires every time the output file grows.
        // We don't overwrite the per-phase label, but it counts as
        // activity and the runner can append a "(writing response)"
        // suffix if it wants stronger signal.
        "agent_writing_output" => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_to_width_passes_through_short_text() {
        assert_eq!(fit_to_width("hello", 80), "hello");
        assert_eq!(fit_to_width("hello", 5), "hello");
    }

    #[test]
    fn fit_to_width_truncates_long_text_with_ellipsis() {
        let long = "abcdefghijklmnopqrstuvwxyz";
        let got = fit_to_width(long, 10);
        // 9 chars + ellipsis
        assert_eq!(got, "abcdefghi…");
        assert_eq!(got.chars().count(), 10);
    }

    #[test]
    fn fit_to_width_handles_tiny_widths() {
        // Below 4 columns the ellipsis-budget collapses; we just take
        // whatever we can fit. Anything ≥ 4 keeps the trailing ellipsis.
        assert_eq!(fit_to_width("hello world", 3), "hel");
        assert_eq!(fit_to_width("hello world", 1), "h");
        assert_eq!(fit_to_width("hello world", 4), "hel…");
    }
}
