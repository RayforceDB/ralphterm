//! Async TTY-native agent driver.
//!
//! This module owns the contract that defines what ralphterm does for a
//! living: spawn the official AI CLI in a real PTY, paste a prompt as
//! keystrokes the way a human would, watch for explicit BEGIN/END
//! wrapper markers to know when the agent considers itself done.
//!
//! ## Done-detection contract
//!
//! The driver prepends a protocol preamble to whatever prompt the caller
//! provides. The preamble instructs the agent to wrap its formal response
//! between two markers that include a per-iteration nonce:
//!
//! ```text
//! <<<RALPHTERM:BEGIN:{nonce}>>>
//! ...response...
//! <<<RALPHTERM:END:{nonce}>>>
//! ```
//!
//! The nonce makes the markers per-iteration unique — old transcripts
//! quoted into a later session can't trigger a false positive, and the
//! agent obviously can't predict a fresh random hex string.
//!
//! State machine:
//!
//! ```text
//! WAITING ──BEGIN──▶ CAPTURING ──END──▶ DONE (send /exit, reap process)
//!    │
//!    └──idle_timeout──▶ kill child, return timed_out=true
//! ```
//!
//! There is no quiescence guessing. Either the BEGIN/END markers arrive
//! and the iteration is genuinely done, or the idle timer fires and the
//! iteration genuinely failed. Both are observable categories.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::runner::{spawn_agent_command_promptless, strip_ansi_escapes, SpawnedAgent};

/// What the caller hands to `drive_agent`.
pub struct AgentSpec<'a> {
    /// The agent command (shlex-parsed by the spawner). Bare `claude`
    /// automatically gets `--dangerously-skip-permissions`; that's the
    /// only flag injection — no `--print`, no `-p`, no argv prompt.
    pub command: &'a str,
    /// The task prompt as the caller built it (usually the vendored
    /// `task.txt` after `{{VAR}}` substitution). The driver prepends a
    /// short protocol preamble to it before pasting.
    pub task_prompt: &'a str,
    /// Kill the agent and return `timed_out = true` if no PTY output
    /// arrives for this long.
    pub idle_timeout: Duration,
    /// Optional cancellation channel so the controller can stop a
    /// running agent (e.g. dashboard "cancel run" button).
    pub cancel: Option<tokio::sync::watch::Receiver<bool>>,
    /// Optional event sink for observability — drive_agent calls this
    /// on each major state transition. Pushes flow into the existing
    /// `RunStore::append_progress_event` pipeline.
    pub event_sink: Option<EventSink>,
}

pub type EventSink = Arc<dyn Fn(DriverEvent) + Send + Sync>;

/// Events emitted at state transitions. Field names are stable; the
/// dashboard front-end reads `kind` directly.
#[derive(Debug, Clone)]
pub struct DriverEvent {
    pub kind: &'static str,
    pub nonce: String,
    pub detail: Option<String>,
}

/// The result of one driven agent invocation.
pub struct AgentRun {
    /// Full PTY transcript, ANSI escapes stripped. Useful for debugging
    /// and for the on-disk `.ralphex/progress/transcripts/...` artifact.
    pub transcript: String,
    /// The text between the BEGIN and END markers, if both arrived. This
    /// is the clean hand-off to the next session (next implementer round,
    /// reviewer, fixer prompt). `None` if the agent didn't produce
    /// matching markers.
    pub captured_response: Option<String>,
    /// Child process exit code (0 if killed before exit was observed).
    pub exit_code: i32,
    /// True when we killed the child after `idle_timeout`.
    pub timed_out: bool,
    /// True when both markers arrived (the agent obeyed protocol).
    pub end_marker_seen: bool,
    /// True when the BEGIN marker arrived (used by callers to
    /// distinguish "agent never started talking" from "agent talked but
    /// never said done").
    pub begin_marker_seen: bool,
    /// The nonce we generated for this iteration. Useful for debugging
    /// and for the dashboard to correlate events.
    pub nonce: String,
}

/// The main entry point. Spawns the agent in a PTY, pastes the prompt
/// with the protocol preamble, runs the BEGIN/END state machine.
pub async fn drive_agent(spec: AgentSpec<'_>) -> Result<AgentRun> {
    let nonce = make_nonce();
    let prompt = build_prompt_with_protocol(spec.task_prompt, &nonce);

    let SpawnedAgent { child, master } =
        spawn_agent_command_promptless(spec.command).context("spawn agent")?;

    // The promptless spawn returns master+child; we own the prompt
    // delivery so we can paste keystrokes after we know the child is
    // alive (and, in a future enhancement, after the REPL ready
    // indicator appears).
    let writer = Arc::new(Mutex::new(master.take_writer().context("take pty writer")?));
    {
        let mut w = writer.lock().expect("writer mutex");
        w.write_all(prompt.as_bytes())
            .context("write task prompt")?;
        w.write_all(b"\n").context("write prompt newline")?;
        w.flush().context("flush prompt")?;
    }

    // Forward the agent's PTY output into an async channel. portable-pty
    // exposes a synchronous std::io::Read; bridge to async via
    // spawn_blocking + tokio::sync::mpsc.
    let reader = master.try_clone_reader().context("clone pty reader")?;
    let (byte_tx, mut byte_rx) = mpsc::channel::<Vec<u8>>(64);
    let (done_tx, mut done_rx) = mpsc::channel::<DriverShutdown>(2);

    let reader_done_tx = done_tx.clone();
    tokio::task::spawn_blocking(move || {
        let mut reader = reader;
        let mut buf = [0u8; 8192];
        loop {
            match std::io::Read::read(&mut reader, &mut buf) {
                Ok(0) => {
                    let _ = reader_done_tx.blocking_send(DriverShutdown::ReaderEof);
                    return;
                }
                Ok(n) => {
                    if byte_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        return;
                    }
                }
                Err(err) => {
                    let _ =
                        reader_done_tx.blocking_send(DriverShutdown::ReaderError(err.to_string()));
                    return;
                }
            }
        }
    });

    // Optional cancellation watcher: pushes a Cancel sentinel when the
    // controller flips the watch to `true`.
    if let Some(mut cancel) = spec.cancel.clone() {
        let done_tx = done_tx.clone();
        tokio::spawn(async move {
            while cancel.changed().await.is_ok() {
                if *cancel.borrow() {
                    let _ = done_tx.send(DriverShutdown::Cancelled).await;
                    return;
                }
            }
        });
    }

    let mut transcript = String::new();
    let mut state = State::Waiting;
    let mut captured = String::new();
    let begin_marker = format!("<<<RALPHTERM:BEGIN:{nonce}>>>");
    let end_marker = format!("<<<RALPHTERM:END:{nonce}>>>");
    let mut leftover = String::new();
    let mut last_byte_at = Instant::now();
    let mut shutdown: Option<DriverShutdown> = None;

    if let Some(sink) = spec.event_sink.as_ref() {
        sink(DriverEvent {
            kind: "agent_started",
            nonce: nonce.clone(),
            detail: None,
        });
    }

    loop {
        let idle_deadline = last_byte_at + spec.idle_timeout;
        let now = Instant::now();
        let wait_for = idle_deadline.saturating_duration_since(now);

        tokio::select! {
            biased;
            done = done_rx.recv() => {
                shutdown = done;
                break;
            }
            chunk = byte_rx.recv() => {
                match chunk {
                    Some(bytes) => {
                        last_byte_at = Instant::now();
                        let chunk_str = String::from_utf8_lossy(&bytes).into_owned();
                        transcript.push_str(&chunk_str);
                        let cleaned = strip_ansi_escapes(&chunk_str);
                        leftover.push_str(&cleaned);

                        let (new_state, new_leftover) = scan_for_markers(
                            state,
                            &mut captured,
                            &leftover,
                            &begin_marker,
                            &end_marker,
                        );
                        state = new_state;
                        leftover = new_leftover;

                        if state == State::Done {
                            if let Some(sink) = spec.event_sink.as_ref() {
                                sink(DriverEvent {
                                    kind: "agent_end_marker",
                                    nonce: nonce.clone(),
                                    detail: None,
                                });
                            }
                            break;
                        }
                    }
                    None => {
                        // Sender side dropped. Treat as EOF; wait for the
                        // done channel to deliver the actual reason.
                        if shutdown.is_none() {
                            shutdown = done_rx.recv().await;
                        }
                        break;
                    }
                }
            }
            _ = sleep(wait_for) => {
                shutdown = Some(DriverShutdown::IdleTimeout);
                break;
            }
        }
    }

    // Teardown. If we saw END, ask the agent to /exit cleanly. Otherwise
    // kill on idle/cancel/error path.
    let timed_out = matches!(shutdown, Some(DriverShutdown::IdleTimeout));
    if state == State::Done {
        let mut w = writer.lock().expect("writer mutex");
        let _ = w.write_all(b"/exit\r");
        let _ = w.flush();
    } else if let Some(reason) = &shutdown {
        if let Some(sink) = spec.event_sink.as_ref() {
            sink(DriverEvent {
                kind: "agent_aborted",
                nonce: nonce.clone(),
                detail: Some(format!("{reason:?}")),
            });
        }
    }

    // Reap the child with a tight timeout. Force-kill on overrun.
    let mut child_for_reap = child;
    let exit_code = tokio::task::spawn_blocking(move || {
        const REAP_BUDGET: Duration = Duration::from_secs(3);
        let deadline = Instant::now() + REAP_BUDGET;
        loop {
            match child_for_reap.try_wait() {
                Ok(Some(status)) => return status.exit_code() as i32,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child_for_reap.kill();
                        let _ = child_for_reap.wait();
                        return -1;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => return -1,
            }
        }
    })
    .await
    .unwrap_or(-1);

    // Drain any leftover bytes the reader thread queued before we
    // observed teardown.
    while let Ok(bytes) = byte_rx.try_recv() {
        let chunk_str = String::from_utf8_lossy(&bytes).into_owned();
        transcript.push_str(&chunk_str);
    }

    let begin_marker_seen = state != State::Waiting;
    let end_marker_seen = state == State::Done;
    let captured_response = if end_marker_seen && !captured.is_empty() {
        Some(captured.trim_end_matches('\n').to_string())
    } else {
        None
    };

    if let Some(sink) = spec.event_sink.as_ref() {
        sink(DriverEvent {
            kind: if end_marker_seen {
                "agent_completed"
            } else if timed_out {
                "agent_timed_out"
            } else {
                "agent_exited_without_end"
            },
            nonce: nonce.clone(),
            detail: None,
        });
    }

    Ok(AgentRun {
        transcript,
        captured_response,
        exit_code,
        timed_out,
        end_marker_seen,
        begin_marker_seen,
        nonce,
    })
}

#[derive(Debug, Clone)]
enum DriverShutdown {
    ReaderEof,
    #[allow(dead_code)] // surfaced via tracing in a future enhancement
    ReaderError(String),
    IdleTimeout,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Waiting,
    Capturing,
    Done,
}

fn build_prompt_with_protocol(task_prompt: &str, nonce: &str) -> String {
    format!(
        "RALPHTERM PROTOCOL — you MUST follow this exactly:\n  When you are about to produce your final response for this iteration, print this line ON ITS OWN:\n      <<<RALPHTERM:BEGIN:{nonce}>>>\n  Inside, write a concise account of: which task you picked, what files you changed, what validation you ran, what should happen next.\n  When the response is complete, print this line ON ITS OWN:\n      <<<RALPHTERM:END:{nonce}>>>\n  Do not produce any output after the END marker.\n\n---\n\n{task_prompt}\n"
    )
}

fn make_nonce() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    format!("{:032x}", nanos ^ (pid << 96))
}

/// Process the rolling cleaned-stream buffer one chunk at a time,
/// transitioning state on marker matches. Returns the new state and the
/// remaining "after the last consumed marker" tail that should keep
/// rolling forward in case the next chunk completes a partial marker.
fn scan_for_markers(
    mut state: State,
    captured: &mut String,
    buffer: &str,
    begin_marker: &str,
    end_marker: &str,
) -> (State, String) {
    let mut cursor = 0usize;
    let bytes = buffer.as_bytes();
    while cursor < bytes.len() {
        match state {
            State::Waiting => {
                if let Some(rel) = buffer[cursor..].find(begin_marker) {
                    let after = cursor + rel + begin_marker.len();
                    // Consume up through the marker.
                    cursor = after;
                    state = State::Capturing;
                } else {
                    // No marker in this buffer; keep rolling forward but
                    // retain enough tail to catch a marker straddling
                    // the chunk boundary.
                    return (state, retain_tail(buffer, begin_marker.len()));
                }
            }
            State::Capturing => {
                if let Some(rel) = buffer[cursor..].find(end_marker) {
                    let chunk = &buffer[cursor..cursor + rel];
                    captured.push_str(chunk);
                    // cursor advance is unused since we return immediately
                    // on Done, but the math is kept here for clarity in
                    // case future code needs to keep parsing afterwards.
                    state = State::Done;
                    return (state, String::new());
                } else {
                    let tail_keep = end_marker.len();
                    let safe_end = if buffer.len() > cursor + tail_keep {
                        buffer.len() - tail_keep
                    } else {
                        cursor
                    };
                    if safe_end > cursor {
                        captured.push_str(&buffer[cursor..safe_end]);
                        return (state, buffer[safe_end..].to_string());
                    }
                    return (state, buffer[cursor..].to_string());
                }
            }
            State::Done => return (state, String::new()),
        }
    }
    (state, String::new())
}

fn retain_tail(buffer: &str, marker_len: usize) -> String {
    let keep = marker_len.saturating_sub(1);
    if buffer.len() <= keep {
        return buffer.to_string();
    }
    let start = buffer.len() - keep;
    // Don't split inside a UTF-8 codepoint.
    let start = (0..=start)
        .rev()
        .find(|i| buffer.is_char_boundary(*i))
        .unwrap_or(0);
    buffer[start..].to_string()
}

/// Re-export needed by callers that want to write to the PTY after
/// drive_agent returns (mid-run input — v0.4). Currently unused.
#[allow(dead_code)]
pub(crate) struct DriverHandle {
    pub writer: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    pub transcript: Arc<Mutex<String>>,
    pub progress_dir: PathBuf,
}

use std::io::Write;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_includes_markers_and_task_text() {
        let p = build_prompt_with_protocol("DO THE THING", "abc123");
        assert!(p.contains("<<<RALPHTERM:BEGIN:abc123>>>"));
        assert!(p.contains("<<<RALPHTERM:END:abc123>>>"));
        assert!(p.contains("DO THE THING"));
        assert!(p.contains("RALPHTERM PROTOCOL"));
    }

    #[test]
    fn nonce_is_unique_across_calls_within_a_process() {
        let a = make_nonce();
        std::thread::sleep(Duration::from_millis(2));
        let b = make_nonce();
        assert_ne!(a, b);
        assert_eq!(a.len(), 32);
        assert_eq!(b.len(), 32);
    }

    #[test]
    fn scan_transitions_waiting_to_capturing_on_begin() {
        let begin = "<<<RALPHTERM:BEGIN:n>>>";
        let end = "<<<RALPHTERM:END:n>>>";
        let mut captured = String::new();
        let (state, tail) = scan_for_markers(
            State::Waiting,
            &mut captured,
            "noise noise\n<<<RALPHTERM:BEGIN:n>>>some response",
            begin,
            end,
        );
        assert_eq!(state, State::Capturing);
        // The post-BEGIN text "some response" is shorter than end_marker.len(),
        // so the scanner holds it ALL back as tail for boundary safety — on
        // the next chunk it will be re-examined alongside fresh bytes. The
        // sum (captured + tail) covers everything past BEGIN.
        let combined = format!("{captured}{tail}");
        assert_eq!(combined, "some response");
    }

    #[test]
    fn scan_transitions_capturing_to_done_on_end() {
        let begin = "<<<RALPHTERM:BEGIN:n>>>";
        let end = "<<<RALPHTERM:END:n>>>";
        let mut captured = String::from("first half ");
        let (state, tail) = scan_for_markers(
            State::Capturing,
            &mut captured,
            "second half<<<RALPHTERM:END:n>>>trailing",
            begin,
            end,
        );
        assert_eq!(state, State::Done);
        assert_eq!(captured, "first half second half");
        assert!(tail.is_empty());
    }

    #[test]
    fn scan_holds_back_tail_when_marker_might_be_split() {
        let begin = "<<<RALPHTERM:BEGIN:n>>>";
        let end = "<<<RALPHTERM:END:n>>>";
        let mut captured = String::new();
        // Buffer ends in part of the BEGIN marker — caller should keep
        // the trailing bytes around for the next chunk.
        let (state, tail) = scan_for_markers(
            State::Waiting,
            &mut captured,
            "noise <<<RALPHTERM:BEGIN",
            begin,
            end,
        );
        assert_eq!(state, State::Waiting);
        assert!(
            tail.contains("BEGIN") || tail.contains("<<<"),
            "tail should retain potential-marker bytes: {tail}"
        );
    }
}
