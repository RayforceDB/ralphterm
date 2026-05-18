//! One-shot manual smoke test for the agent driver.
//!
//! Usage:
//!   cd /tmp/some-trusted-dir
//!   ralphterm-test-driver "what is 2+2? respond very briefly."
//!
//! Spawns bare `claude` via the agent_driver, expects it to wrap its
//! response between BEGIN/END markers, prints what we captured.
//!
//! Prereqs:
//!   - `claude` on PATH and authenticated.
//!   - Workspace must be trusted by Claude Code (run `claude` once
//!     manually and accept the trust dialog).
//!
//! This is intentionally a separate binary so we don't pollute the main
//! ralphterm CLI surface with debug entrypoints.

use std::sync::Arc;
use std::time::Duration;

use ralphterm::agent_driver::{drive_agent, AgentSpec, DriverEvent};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let task_prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "what is 2+2? respond very briefly.".to_string());

    eprintln!("[test_driver] cwd={}", std::env::current_dir()?.display());
    eprintln!("[test_driver] task prompt: {task_prompt}");
    eprintln!("[test_driver] spawning bare `claude` interactively…");
    eprintln!();

    let sink: Arc<dyn Fn(DriverEvent) + Send + Sync> = Arc::new(|ev: DriverEvent| {
        eprintln!("[event] {} detail={:?}", ev.kind, ev.detail);
    });

    let repo_root = std::env::current_dir()?;
    let started = std::time::Instant::now();
    let run = drive_agent(AgentSpec {
        command: "claude",
        task_prompt: &task_prompt,
        repo_root: &repo_root,
        idle_timeout: Duration::from_secs(180),
        cancel: None,
        event_sink: Some(sink),
    })
    .await?;
    let elapsed = started.elapsed();

    eprintln!();
    eprintln!("[test_driver] elapsed       : {:?}", elapsed);
    eprintln!("[test_driver] exit_code     : {}", run.exit_code);
    eprintln!("[test_driver] timed_out     : {}", run.timed_out);
    eprintln!("[test_driver] done_via_file : {}", run.done_via_file);
    eprintln!("[test_driver] crashed_early : {}", run.crashed_before_done);
    eprintln!("[test_driver] cancelled     : {}", run.cancelled);
    eprintln!(
        "[test_driver] output_path   : {}",
        run.output_path.display()
    );
    eprintln!("[test_driver] nonce         : {}", run.nonce);
    eprintln!(
        "[test_driver] transcript len: {} bytes",
        run.transcript.len()
    );
    eprintln!();
    eprintln!("--- captured_response ---");
    match &run.captured_response {
        Some(text) => println!("{}", text),
        None => eprintln!("(none — agent did not emit BEGIN/END markers)"),
    }
    eprintln!();
    eprintln!("--- transcript (ANSI escapes preserved, hex-dumped tail) ---");
    let tail_bytes = run.transcript.as_bytes();
    let start = tail_bytes.len().saturating_sub(800);
    eprintln!("{}", String::from_utf8_lossy(&tail_bytes[start..]));
    eprintln!();
    eprintln!("--- transcript hex (last 400 bytes) ---");
    let hex_start = tail_bytes.len().saturating_sub(400);
    for chunk in tail_bytes[hex_start..].chunks(32) {
        eprint!("  ");
        for b in chunk {
            eprint!("{:02x} ", b);
        }
        eprintln!();
    }

    if !run.done_via_file {
        std::process::exit(1);
    }
    Ok(())
}
