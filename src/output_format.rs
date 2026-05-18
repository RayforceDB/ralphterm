use std::path::Path;
use std::time::Duration;

pub fn print_version_banner() {
    println!("ralphterm v{}", env!("CARGO_PKG_VERSION"));
}

pub fn print_branch_creating(branch: &str) {
    println!("creating branch: {branch}");
}

pub fn print_task_execution_completed() {
    // Ralphex prefixes this line with its agent-narration timestamp; we
    // mirror the format so the side-by-side diff stays clean.
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    println!("[{ts}] task execution completed successfully");
}

/// Heartbeat printed when a review phase starts. Without this the user
/// sees several minutes of dead terminal while parallel reviewer agents
/// run silently.
pub fn print_phase_start(label: &str) {
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    println!("[{ts}] {label} — running...");
}

pub fn print_phase_done(label: &str, elapsed: Duration) {
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    println!("[{ts}] {label} — done in {}s", elapsed.as_secs());
}

pub fn print_run_header(
    max_iterations: usize,
    mode_label: &str,
    plan_path: &Path,
    branch: &str,
    progress_log: &Path,
) {
    println!("starting ralphex loop (max {max_iterations} iterations) ({mode_label})");
    println!("plan: {}", plan_path.display());
    println!("branch: {branch}");
    println!("progress log: {}", progress_log.display());
    println!();
}

pub fn print_task_phase_start() {
    println!("starting task execution phase");
    println!();
}

pub fn print_iteration_header(n: usize) {
    println!("--- task iteration {n} ---");
}

pub fn print_review_phase_start(label: &str) {
    println!();
    println!("{label}");
}

pub fn print_completion_summary(
    elapsed: Duration,
    files: usize,
    additions: usize,
    deletions: usize,
    plan_dest: &Path,
    branch: &str,
    progress_log: &Path,
) {
    println!();
    println!(
        "completed in {}s ({} files, +{}/-{} lines)",
        elapsed.as_secs(),
        files,
        additions,
        deletions
    );
    println!("  plan: {}", plan_dest.display());
    println!("  branch: {branch}");
    println!("  progress log: {}", progress_log.display());
}

pub fn print_moved_plan(dest: &Path) {
    println!("moved plan to {}", dest.display());
}

pub fn print_all_tasks_completed() {
    println!("all tasks completed, starting code review...");
}

pub fn mode_label(tasks_only: bool, review_only: bool, external_only: bool) -> &'static str {
    if tasks_only {
        "tasks-only mode"
    } else if review_only {
        "review-only mode"
    } else if external_only {
        "external-only mode"
    } else {
        "full mode"
    }
}
