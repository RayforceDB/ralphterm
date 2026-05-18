//! Async TTY-native agent driver.
//!
//! ralphterm's reason to exist: spawn the official AI CLI in a real PTY
//! the way a human does, paste the prompt as keystrokes, drive any
//! interactive dialogs the agent shows. This module is the contract.
//!
//! ## Why file-handoff, not in-PTY markers
//!
//! An earlier design asked the agent to wrap its response between inline
//! BEGIN/END tokens emitted to stdout. That fails against real claude
//! because claude's REPL renders the user's prompt back into its
//! conversation UI — so the marker tokens we put in our own preamble
//! get echoed through the reader BEFORE claude even produces a
//! response, and the scanner false-positives on the echo.
//!
//! The fix is to route the response through a SIDE CHANNEL the agent's
//! TUI doesn't touch: a file. We tell the agent to write its response
//! to a unique path with BEGIN/END markers wrapping the contents. The
//! driver polls that path. When the file contains a valid END marker,
//! the response is done. The PTY stream is used only for transcript
//! capture, dialog driving, and process-exit observation — not for
//! determining "done".
//!
//! ## Done-detection (formal)
//!
//! `WAITING` → `DONE` when ANY of:
//!   - The output file exists AND contains the END marker (success)
//!   - The child process exits (success ONLY if file is complete; else
//!     `crashed_before_done = true`)
//!   - Idle timeout elapses with no PTY output (failure;
//!     `timed_out = true`; partial output file preserved)
//!   - Cancellation channel flipped (failure; `cancelled = true`)
//!
//! After DONE we send `/exit\r` to give claude a clean shutdown and
//! reap the child with a 3s budget; force-kill on overrun.
//!
//! ## Dialog driving (built-in)
//!
//! Before pasting the prompt, the driver watches for claude's one-time
//! per-workspace "Bypass Permissions" safety-acceptance dialog and
//! sends down-arrow + Enter to accept. This is exactly what ralphterm
//! exists to do.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::runner::{spawn_agent_command_promptless_with_env, strip_ansi_escapes, SpawnedAgent};

/// What the caller hands to `drive_agent`.
pub struct AgentSpec<'a> {
    /// The agent command (shlex-parsed by the spawner). Bare `claude`
    /// gets `--permission-mode bypassPermissions` auto-injected. No
    /// `--print`, no `-p`, no argv prompt.
    pub command: &'a str,
    /// The caller's task prompt (typically the vendored task.txt after
    /// `{{VAR}}` substitution). The driver prepends a file-handoff
    /// protocol preamble before pasting.
    pub task_prompt: &'a str,
    /// Repository root. Used to derive the output-file path so it lives
    /// inside the workspace claude is permitted to write to.
    pub repo_root: &'a Path,
    /// Kill the agent and fail if no PTY output AND no file update for
    /// this long.
    pub idle_timeout: Duration,
    /// Optional cancellation channel for "stop this run" from the
    /// dashboard or a signal handler.
    pub cancel: Option<tokio::sync::watch::Receiver<bool>>,
    /// Optional event sink for observability — drive_agent calls this
    /// at major state transitions.
    pub event_sink: Option<EventSink>,
}

pub type EventSink = Arc<dyn Fn(DriverEvent) + Send + Sync>;

/// Events emitted at state transitions.
#[derive(Debug, Clone)]
pub struct DriverEvent {
    pub kind: &'static str,
    pub nonce: String,
    pub detail: Option<String>,
}

/// The result of one driven agent invocation.
pub struct AgentRun {
    /// Full PTY transcript (ANSI escapes preserved — caller strips
    /// if it wants). Useful for debugging.
    pub transcript: String,
    /// The text between the file's BEGIN and END markers. `None` if the
    /// agent never produced a complete output file.
    pub captured_response: Option<String>,
    /// Absolute path of the output file we asked the agent to write.
    /// Preserved on disk even on failure paths for debugging.
    pub output_path: PathBuf,
    pub exit_code: i32,
    pub timed_out: bool,
    pub cancelled: bool,
    /// True when the child exited before we observed a complete output
    /// file. Distinct from `timed_out`: a crashed agent may have died
    /// quickly without ever pasting anything.
    pub crashed_before_done: bool,
    /// True when the file was found with a valid END marker.
    pub done_via_file: bool,
    /// The nonce we generated for this iteration. Embedded in the
    /// output filename so multiple parallel iterations don't collide.
    pub nonce: String,
}

/// The main entry point.
pub async fn drive_agent(spec: AgentSpec<'_>) -> Result<AgentRun> {
    let nonce = make_nonce();
    let output_dir = spec.repo_root.join(".ralphterm").join("iteration-output");
    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("create {}", output_dir.display()))?;
    let output_path = output_dir.join(format!("{nonce}.md"));
    let prompt_path = output_dir.join(format!("{nonce}.prompt.txt"));
    let transcript_path = output_dir.join(format!("{nonce}.transcript.txt"));

    // ALWAYS write the full inline-form wrapped prompt to disk so the
    // agent (or a curious operator) can read it. The pasted prompt may
    // either be this full text (small prompts) or a short pointer to
    // this file (large prompts) — see build_prompt_with_protocol.
    let inline_prompt = build_inline_prompt_with_protocol(spec.task_prompt, &nonce, &output_path);
    std::fs::write(&prompt_path, &inline_prompt)
        .with_context(|| format!("write {}", prompt_path.display()))?;
    let prompt = build_prompt_with_protocol(spec.task_prompt, &nonce, &output_path, &prompt_path);
    // Touch the transcript so `tail -f` works the moment the run starts.
    let _ = std::fs::File::create(&transcript_path);

    // Expose paths to the spawned agent so non-interactive fixtures (or
    // headless wrappers) can satisfy the file-handoff contract without
    // having to parse the bracketed-paste prompt from PTY stdin. Real
    // claude ignores unknown env vars.
    let output_path_str = output_path.to_string_lossy().into_owned();
    let prompt_path_str = prompt_path.to_string_lossy().into_owned();
    let nonce_env = nonce.clone();
    let env = [
        ("RALPHTERM_OUTPUT_FILE", output_path_str.as_str()),
        ("RALPHTERM_PROMPT_FILE", prompt_path_str.as_str()),
        ("RALPHTERM_NONCE", nonce_env.as_str()),
    ];

    let SpawnedAgent { child, master } =
        spawn_agent_command_promptless_with_env(spec.command, &env).context("spawn agent")?;

    let writer = Arc::new(Mutex::new(master.take_writer().context("take pty writer")?));

    // Bridge the blocking std::io::Read PTY reader into an async channel.
    let reader = master.try_clone_reader().context("clone pty reader")?;
    let (byte_tx, mut byte_rx) = mpsc::channel::<Vec<u8>>(64);
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<DriverShutdown>(2);

    let reader_shutdown_tx = shutdown_tx.clone();
    tokio::task::spawn_blocking(move || {
        let mut reader = reader;
        let mut buf = [0u8; 8192];
        loop {
            match std::io::Read::read(&mut reader, &mut buf) {
                Ok(0) => {
                    let _ = reader_shutdown_tx.blocking_send(DriverShutdown::ReaderEof);
                    return;
                }
                Ok(n) => {
                    if byte_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        return;
                    }
                }
                Err(err) => {
                    let _ = reader_shutdown_tx
                        .blocking_send(DriverShutdown::ReaderError(err.to_string()));
                    return;
                }
            }
        }
    });

    // Cancellation watcher.
    if let Some(mut cancel) = spec.cancel.clone() {
        let shutdown_tx = shutdown_tx.clone();
        tokio::spawn(async move {
            while cancel.changed().await.is_ok() {
                if *cancel.borrow() {
                    let _ = shutdown_tx.send(DriverShutdown::Cancelled).await;
                    return;
                }
            }
        });
    }

    let mut transcript = String::new();
    if let Some(sink) = spec.event_sink.as_ref() {
        sink(DriverEvent {
            kind: "agent_started",
            nonce: nonce.clone(),
            detail: Some(format!("output_path={}", output_path.display())),
        });
    }

    // Phase 1: wait for claude to be ready (alt-screen-buffer signal,
    // or 5s fallback), and drive the bypass-permissions dialog if it
    // appears. The wait deliberately consumes whatever startup noise
    // and dialog content the TTY produces so the steady-state reader
    // loop starts from a clean point.
    wait_for_repl_ready(&mut transcript, &mut byte_rx, &writer, &spec).await?;

    // Phase 2: paste the prompt now that claude's REPL is alive and the
    // safety dialog (if any) is past.
    //
    // Two-step submission: we deliver the body via bracketed-paste mode
    // (xterm DECSET 2004), then sleep briefly so claude's TUI processes
    // the paste, then send a separate CR (`\r`) for the submit.
    // Sending them in one flush sometimes interleaves with claude's own
    // TUI redraw and the submit gets lost.
    //
    // TUI input subtleties:
    //   - Claude's input box treats Enter (CR `\r`) as "submit message"
    //     and uses Shift+Enter for "newline within message". A raw LF
    //     (`\n`) embedded in our paste would normally get dropped.
    //   - Bracketed paste mode tells the TUI "this block between
    //     ESC[200~ and ESC[201~ is one atomic paste — preserve
    //     internal newlines as message content, not keystrokes."
    //   - We log the paste via the event sink so failures are visible.
    {
        let mut w = writer.lock().expect("writer mutex");
        w.write_all(b"\x1b[200~").context("paste start")?;
        w.write_all(prompt.as_bytes())
            .context("write task prompt")?;
        w.write_all(b"\x1b[201~").context("paste end")?;
        w.flush().context("flush paste")?;
    }
    if let Some(sink) = spec.event_sink.as_ref() {
        sink(DriverEvent {
            kind: "agent_prompt_pasted",
            nonce: nonce.clone(),
            detail: Some(format!("{} bytes", prompt.len())),
        });
    }
    sleep(Duration::from_millis(200)).await;
    {
        let mut w = writer.lock().expect("writer mutex");
        w.write_all(b"\r").context("submit prompt")?;
        w.flush().context("flush submit")?;
    }
    if let Some(sink) = spec.event_sink.as_ref() {
        sink(DriverEvent {
            kind: "agent_prompt_submitted",
            nonce: nonce.clone(),
            detail: None,
        });
    }

    // Phase 3: main loop. Three concurrent signals decide we're done:
    //   - File watchdog finds END marker in the output file
    //   - Child process exits
    //   - Idle timer elapses with no PTY output activity (failure)
    let end_marker = "<<<END>>>";
    let mut last_byte_at = Instant::now();
    let mut last_data_event_at = Instant::now();
    let mut shutdown: Option<DriverShutdown> = None;
    let mut file_complete = false;
    let mut output_file_seen_growing = false;
    let mut last_output_file_size: u64 = 0;
    let mut file_check_tick = tokio::time::interval(Duration::from_millis(200));
    file_check_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        let idle_deadline = last_byte_at + spec.idle_timeout;
        let now = Instant::now();
        let idle_wait = idle_deadline.saturating_duration_since(now);

        tokio::select! {
            biased;
            done = shutdown_rx.recv() => {
                shutdown = done;
                break;
            }
            chunk = byte_rx.recv() => {
                match chunk {
                    Some(bytes) => {
                        last_byte_at = Instant::now();
                        transcript.push_str(&String::from_utf8_lossy(&bytes));
                        // Mirror the raw PTY bytes to a side-file the
                        // operator can `tail -f` while the run is in
                        // flight. Useful for distinguishing "agent in
                        // silent tool-use" from "agent actually stuck".
                        if let Ok(mut f) = std::fs::OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open(&transcript_path)
                        {
                            let _ = std::io::Write::write_all(&mut f, &bytes);
                        }
                        // Throttled heartbeat: emit at most once per
                        // 2 s while the agent is streaming. The spinner
                        // uses this to refresh its "last byte Ns ago"
                        // liveness indicator without flooding event
                        // sinks on every 8 KiB chunk.
                        if last_byte_at.duration_since(last_data_event_at)
                            >= Duration::from_secs(2)
                        {
                            last_data_event_at = last_byte_at;
                            if let Some(sink) = spec.event_sink.as_ref() {
                                sink(DriverEvent {
                                    kind: "agent_data",
                                    nonce: nonce.clone(),
                                    detail: Some(format!("{} bytes", transcript.len())),
                                });
                            }
                        }
                    }
                    None => {
                        // Reader sender dropped without sending shutdown.
                        // Treat as EOF.
                        if shutdown.is_none() {
                            shutdown = shutdown_rx.recv().await;
                        }
                        break;
                    }
                }
            }
            _ = file_check_tick.tick() => {
                // Surface output-file growth as a separate event so the
                // spinner can show "writing response" before the END
                // marker lands. This is the strongest "still working"
                // signal during long claude tool-use phases that don't
                // print anything to the PTY.
                if let Ok(size) = std::fs::metadata(&output_path).map(|m| m.len()) {
                    if size > 0 && size != last_output_file_size {
                        last_output_file_size = size;
                        if !output_file_seen_growing {
                            output_file_seen_growing = true;
                        }
                        if let Some(sink) = spec.event_sink.as_ref() {
                            sink(DriverEvent {
                                kind: "agent_writing_output",
                                nonce: nonce.clone(),
                                detail: Some(format!("{size} bytes")),
                            });
                        }
                    }
                }
                if output_file_has_end(&output_path, end_marker) {
                    file_complete = true;
                    if let Some(sink) = spec.event_sink.as_ref() {
                        sink(DriverEvent {
                            kind: "agent_output_file_complete",
                            nonce: nonce.clone(),
                            detail: None,
                        });
                    }
                    break;
                }
            }
            _ = sleep(idle_wait) => {
                shutdown = Some(DriverShutdown::IdleTimeout);
                break;
            }
        }
    }

    let cancelled = matches!(shutdown, Some(DriverShutdown::Cancelled));
    let timed_out = matches!(shutdown, Some(DriverShutdown::IdleTimeout));
    let child_died_early = !file_complete
        && matches!(
            shutdown,
            Some(DriverShutdown::ReaderEof) | Some(DriverShutdown::ReaderError(_))
        );

    // Teardown: send /exit to ask for graceful shutdown if the agent is
    // still around, then reap with a 3s budget.
    if file_complete {
        let mut w = writer.lock().expect("writer mutex");
        let _ = w.write_all(b"/exit\r");
        let _ = w.flush();
    }
    drop(writer); // release the mutex BEFORE the blocking reap
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

    // Drain trailing PTY bytes.
    while let Ok(bytes) = byte_rx.try_recv() {
        transcript.push_str(&String::from_utf8_lossy(&bytes));
    }
    // Final file check — claude may have finished writing during the
    // reap window.
    if !file_complete && output_file_has_end(&output_path, end_marker) {
        file_complete = true;
    }

    let captured_response = if file_complete {
        read_between_markers(&output_path).ok()
    } else {
        None
    };

    if let Some(sink) = spec.event_sink.as_ref() {
        let kind = if cancelled {
            "agent_cancelled"
        } else if timed_out {
            "agent_timed_out"
        } else if file_complete {
            "agent_completed"
        } else if child_died_early {
            "agent_crashed_before_done"
        } else {
            "agent_exited_without_file"
        };
        sink(DriverEvent {
            kind,
            nonce: nonce.clone(),
            detail: None,
        });
    }

    Ok(AgentRun {
        transcript,
        captured_response,
        output_path,
        exit_code,
        timed_out,
        cancelled,
        crashed_before_done: !file_complete && child_died_early,
        done_via_file: file_complete,
        nonce,
    })
}

#[derive(Debug, Clone)]
enum DriverShutdown {
    ReaderEof,
    #[allow(dead_code)]
    ReaderError(String),
    IdleTimeout,
    Cancelled,
}

/// Soft cap on inline-pasted prompts (bytes). Above this, we paste a
/// short instruction that points at the prompt file on disk instead.
/// 8 KiB is well under typical PTY buffer limits and observed to be a
/// safe paste size for claude's REPL.
const INLINE_PASTE_SOFT_CAP: usize = 8 * 1024;

/// The full inline form of the wrapped prompt. Always written to
/// `prompt_path` so the file-reference path has something to read.
fn build_inline_prompt_with_protocol(task_prompt: &str, nonce: &str, output_path: &Path) -> String {
    format!(
        "RALPHTERM PROTOCOL — you MUST follow this exactly:\n\
         When you have a final response for this iteration, write the response to this file:\n\
             {path}\n\
         The file MUST start with the literal line:\n\
             <<<BEGIN>>>\n\
         followed by your response (a concise account of: which task you picked, what files you changed, what validation you ran, what should happen next), and end with the literal line:\n\
             <<<END>>>\n\
         Both markers must be on their own lines. After writing the file you do not need to print anything special — the orchestrator polls the file. (Reference nonce: {nonce})\n\n\
         ---\n\n\
         {task}\n",
        path = output_path.display(),
        task = task_prompt,
    )
}

fn build_prompt_with_protocol(
    task_prompt: &str,
    nonce: &str,
    output_path: &Path,
    prompt_path: &Path,
) -> String {
    // For large prompts (composite multi-dimension reviews especially),
    // pasting tens of kilobytes through bracketed-paste is unreliable —
    // user reproed a 1-hour silent hang on a ~30 KB composite prompt
    // (claude's REPL stopped responding after the paste; zero PTY
    // bytes for 24+ minutes idle). Above 8 KiB we paste a tiny
    // instruction that tells the agent to read the prompt from disk
    // instead. The agent then reads the file via its own tool, which
    // is a single short request that doesn't blow any input buffer.
    if task_prompt.len() > INLINE_PASTE_SOFT_CAP {
        return format!(
            "RALPHTERM PROTOCOL — you MUST follow this exactly:\n\n\
             1. Read your task instructions from this file (it's too large to paste inline):\n\
                    {prompt}\n\
             2. Perform the task it describes.\n\
             3. When you have a final response, write it to this file:\n\
                    {out}\n\
                The file MUST start with the literal line `<<<BEGIN>>>` on its own line, \
                followed by your response (a concise account of what was done), and end with \
                the literal line `<<<END>>>` on its own line. After writing the file you do \
                not need to print anything special — the orchestrator polls the file. \
                (Reference nonce: {nonce})\n",
            prompt = prompt_path.display(),
            out = output_path.display(),
        );
    }
    build_inline_prompt_with_protocol(task_prompt, nonce, output_path)
}

fn make_nonce() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    format!("{:032x}", nanos ^ (pid << 96))
}

/// Read the output file and check whether the END marker is present on
/// its own line. Cheap to call repeatedly (200 ms polling). Missing
/// file is treated as not-yet-complete (returns false).
fn output_file_has_end(path: &Path, end_marker: &str) -> bool {
    let Ok(body) = std::fs::read_to_string(path) else {
        return false;
    };
    body.lines().any(|line| line.trim() == end_marker)
}

/// Extract the text strictly between the first `<<<BEGIN>>>` line and
/// the first subsequent `<<<END>>>` line. Trailing newline trimmed.
fn read_between_markers(path: &Path) -> Result<String> {
    let body = std::fs::read_to_string(path)
        .with_context(|| format!("read iteration output {}", path.display()))?;
    let mut inside = false;
    let mut out = String::new();
    for line in body.lines() {
        if line.trim() == "<<<BEGIN>>>" {
            inside = true;
            continue;
        }
        if line.trim() == "<<<END>>>" {
            break;
        }
        if inside {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out.trim_end_matches('\n').to_string())
}

/// Wait for claude's REPL to be ready before pasting the prompt: look
/// for the alt-screen-buffer DECSET sequence as a positive signal that
/// claude has finished its termios setup, OR fall through after a
/// 5-second timeout. Along the way, drive the one-time per-workspace
/// "Bypass Permissions" safety dialog by sending down-arrow + Enter.
async fn wait_for_repl_ready(
    transcript: &mut String,
    byte_rx: &mut mpsc::Receiver<Vec<u8>>,
    writer: &Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    spec: &AgentSpec<'_>,
) -> Result<()> {
    const READY_DEADLINE: Duration = Duration::from_secs(5);
    const BYPASS_DIALOG_DEADLINE: Duration = Duration::from_secs(4);
    let alt_screen_sequence = "\x1b[?1049h";
    let ready_at = Instant::now() + READY_DEADLINE;
    let bypass_at = Instant::now() + BYPASS_DIALOG_DEADLINE;
    let mut dialog_dismissed = false;

    while Instant::now() < ready_at {
        // Dialog check — only fires once.
        if !dialog_dismissed && Instant::now() < bypass_at {
            let cleaned = strip_ansi_escapes(transcript).to_ascii_lowercase();
            if cleaned.contains("responsibility") && cleaned.contains("permissions") {
                if let Some(sink) = spec.event_sink.as_ref() {
                    sink(DriverEvent {
                        kind: "bypass_permissions_dialog_seen",
                        nonce: "n/a".to_string(),
                        detail: None,
                    });
                }
                {
                    let mut w = writer.lock().expect("writer mutex");
                    w.write_all(b"\x1b[B").context("down arrow")?;
                    w.flush().ok();
                }
                sleep(Duration::from_millis(120)).await;
                {
                    let mut w = writer.lock().expect("writer mutex");
                    w.write_all(b"\r").context("enter")?;
                    w.flush().ok();
                }
                dialog_dismissed = true;
                // Drain claude's transition into REPL.
                let drain_until = Instant::now() + Duration::from_millis(2000);
                while Instant::now() < drain_until {
                    tokio::select! {
                        chunk = byte_rx.recv() => match chunk {
                            Some(bytes) => transcript.push_str(&String::from_utf8_lossy(&bytes)),
                            None => return Ok(()),
                        },
                        _ = sleep(Duration::from_millis(250)) => break,
                    }
                }
                continue;
            }
        }

        // Alt-screen-buffer signal — claude has finished termios setup.
        if transcript.contains(alt_screen_sequence) {
            // Belt-and-braces 150 ms grace.
            sleep(Duration::from_millis(150)).await;
            return Ok(());
        }

        // Otherwise pull more bytes and loop.
        let now = Instant::now();
        let poll_step = if dialog_dismissed {
            Duration::from_millis(200)
        } else {
            Duration::from_millis(150)
        };
        let wait = ready_at.saturating_duration_since(now).min(poll_step);
        tokio::select! {
            chunk = byte_rx.recv() => match chunk {
                Some(bytes) => transcript.push_str(&String::from_utf8_lossy(&bytes)),
                None => return Ok(()),
            },
            _ = sleep(wait) => {}
        }
    }

    Ok(())
}

use std::io::Write;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_includes_path_and_task_text() {
        let p = build_prompt_with_protocol(
            "DO THE THING",
            "abc123",
            Path::new("/tmp/x/abc123.md"),
            Path::new("/tmp/x/abc123.prompt.txt"),
        );
        assert!(p.contains("/tmp/x/abc123.md"));
        assert!(p.contains("<<<BEGIN>>>"));
        assert!(p.contains("<<<END>>>"));
        assert!(p.contains("DO THE THING"));
        assert!(p.contains("RALPHTERM PROTOCOL"));
        assert!(p.contains("abc123"));
    }

    #[test]
    fn build_prompt_uses_file_reference_for_large_prompts() {
        // Reproduces the user's 24-minute-silent-hang scenario: a
        // composite multi-dimension review prompt that's tens of KB.
        // Above INLINE_PASTE_SOFT_CAP we paste a short pointer to the
        // prompt file instead of the prompt body.
        let huge = "X".repeat(INLINE_PASTE_SOFT_CAP + 1);
        let p = build_prompt_with_protocol(
            &huge,
            "abc123",
            Path::new("/tmp/x/abc123.md"),
            Path::new("/tmp/x/abc123.prompt.txt"),
        );
        // Pasted form should be tiny (instruction only, no body).
        assert!(
            p.len() < 1024,
            "large-prompt form should be a short pointer, got {} bytes",
            p.len()
        );
        assert!(p.contains("/tmp/x/abc123.prompt.txt"));
        assert!(p.contains("/tmp/x/abc123.md"));
        assert!(!p.contains(&huge));
    }

    #[test]
    fn nonce_is_unique_across_calls_within_a_process() {
        let a = make_nonce();
        std::thread::sleep(Duration::from_millis(2));
        let b = make_nonce();
        assert_ne!(a, b);
        assert_eq!(a.len(), 32);
    }

    #[test]
    fn output_file_has_end_returns_true_when_marker_on_own_line() {
        let tmp = std::env::temp_dir().join(format!(
            "rt-driver-end-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&tmp, "<<<BEGIN>>>\nbody\n<<<END>>>\n").unwrap();
        assert!(output_file_has_end(&tmp, "<<<END>>>"));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn output_file_has_end_returns_false_when_marker_inline_only() {
        let tmp = std::env::temp_dir().join(format!(
            "rt-driver-end-inline-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&tmp, "we mentioned <<<END>>> inline").unwrap();
        assert!(!output_file_has_end(&tmp, "<<<END>>>"));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn read_between_markers_extracts_body() {
        let tmp = std::env::temp_dir().join(format!(
            "rt-driver-read-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &tmp,
            "preface\n<<<BEGIN>>>\nline 1\nline 2\n<<<END>>>\ntrailing\n",
        )
        .unwrap();
        let body = read_between_markers(&tmp).unwrap();
        assert_eq!(body, "line 1\nline 2");
        let _ = std::fs::remove_file(&tmp);
    }
}
