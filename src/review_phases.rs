use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;

use crate::prompts::{substitute, Prompts};

#[derive(Debug, Clone)]
pub enum ReviewOutcome {
    Pass,
    Issues(Vec<String>),
}

pub struct FirstReviewArgs<'a> {
    pub prompts: &'a Prompts,
    pub reviewer_command: &'a str,
    pub plan_path: &'a std::path::Path,
    pub progress_path: &'a std::path::Path,
    pub default_branch: &'a str,
    pub agent_timeout: Duration,
}

const FIRST_REVIEW_AGENTS: &[&str] = &[
    "quality",
    "implementation",
    "testing",
    "simplification",
    "documentation",
];

pub async fn first_review(args: FirstReviewArgs<'_>) -> Result<ReviewOutcome> {
    run_composite_review(args, FIRST_REVIEW_AGENTS, "phase 1 first review").await
}

const SECOND_REVIEW_AGENTS: &[&str] = &["quality", "implementation"];

pub async fn second_review(args: FirstReviewArgs<'_>) -> Result<ReviewOutcome> {
    run_composite_review(args, SECOND_REVIEW_AGENTS, "phase 3 second review").await
}

/// Spawn ONE reviewer session and hand it a composite prompt that asks
/// the agent to evaluate every dimension (quality, implementation,
/// testing, …) in turn. The agent decides how to organise its own
/// passes. This replaces the old N-parallel design which violated the
/// "one client, one session" expectation of the upstream AI APIs and
/// reliably hit rate limits.
async fn run_composite_review(
    args: FirstReviewArgs<'_>,
    agent_names: &[&str],
    phase_label: &str,
) -> Result<ReviewOutcome> {
    let plan_str = args.plan_path.to_string_lossy().to_string();
    let progress_str = args.progress_path.to_string_lossy().to_string();
    let default_branch = args.default_branch.to_string();
    let repo_root = std::env::current_dir()?;

    let spinner = crate::spinner::Spinner::start(format!(
        "{phase_label}: composing prompt for {} review dimensions",
        agent_names.len()
    ));

    let composite_prompt = build_composite_review_prompt(
        &args.prompts.review_first,
        agent_names,
        |name| args.prompts.agents.get(name).cloned().unwrap_or_default(),
        &plan_str,
        &progress_str,
        &default_branch,
    );

    if let Some(s) = spinner.as_ref() {
        s.set_label(format!(
            "{phase_label}: reviewer running ({} dimensions in one session) — tail .ralphterm/iteration-output/<nonce>.transcript.txt to watch",
            agent_names.len()
        ));
    }

    let bumper: Option<crate::agent_driver::EventSink> = spinner.as_ref().map(|s| {
        let s = s.clone();
        let sink: crate::agent_driver::EventSink = Arc::new(move |_ev| s.bump_activity());
        sink
    });

    let run = crate::agent_driver::drive_agent(crate::agent_driver::AgentSpec {
        command: args.reviewer_command,
        task_prompt: &composite_prompt,
        repo_root: &repo_root,
        idle_timeout: args.agent_timeout,
        cancel: None,
        event_sink: bumper,
    })
    .await?;

    if let Some(s) = spinner.as_ref() {
        s.stop();
    }
    drop(spinner);

    if run.timed_out {
        anyhow::bail!("{phase_label} reviewer timed out");
    }

    let transcript = run.captured_response.unwrap_or_else(|| {
        crate::runner::transcript_without_prompt_echo(&run.transcript, &composite_prompt)
    });

    let mut findings: Vec<String> = Vec::new();
    if transcript_has_critical_issues(&transcript) {
        findings.push(first_line_of_findings(&transcript));
    }

    if findings.is_empty() {
        Ok(ReviewOutcome::Pass)
    } else {
        Ok(ReviewOutcome::Issues(findings))
    }
}

/// Build the single composite prompt that instructs the reviewer agent
/// to walk through each review dimension in turn. Substitutes the
/// per-dimension AGENT_INSTRUCTIONS bodies into the shared template
/// for each dimension and concatenates the results with clear section
/// headers so the agent can organise its own passes without ralphterm
/// having to spawn multiple sessions.
fn build_composite_review_prompt(
    review_template: &str,
    agent_names: &[&str],
    agent_template_for: impl Fn(&str) -> String,
    plan_str: &str,
    progress_str: &str,
    default_branch: &str,
) -> String {
    let dimensions = agent_names.join(", ");
    let mut composite = String::new();
    composite.push_str(&format!(
        "# MULTI-DIMENSION REVIEW (single session, one client)\n\n\
         You are the sole reviewer for this run. Evaluate the work along {n} dimensions: {dimensions}. \
         Decide internally how to organise these passes (serial, interleaved, batched) — \
         the orchestrator does NOT spawn additional sessions on your behalf, by design \
         (one-client-one-session, no parallel API calls).\n\n\
         For each dimension below, perform the review as specified. After all dimensions are \
         done, write a SINGLE consolidated finding list. If ANY dimension surfaces critical or \
         major issues, the run is blocked.\n\n\
         ---\n\n",
        n = agent_names.len(),
        dimensions = dimensions
    ));

    for name in agent_names {
        let agent_body = agent_template_for(name);
        let mut vars: HashMap<&str, &str> = HashMap::new();
        vars.insert("PLAN_FILE", plan_str);
        vars.insert("PROGRESS_FILE", progress_str);
        vars.insert("DEFAULT_BRANCH", default_branch);
        vars.insert("AGENT_NAME", name);
        vars.insert("AGENT_INSTRUCTIONS", agent_body.as_str());
        let pass_prompt = substitute(review_template, &vars);
        composite.push_str(&format!(
            "## Dimension: {name}\n\n{pass_prompt}\n\n---\n\n",
            name = name
        ));
    }

    composite
}

/// Decide whether an external (codex) reviewer transcript says the
/// codebase is clean. The vendored codex_review.txt template asks the
/// reviewer to say "NO ISSUES FOUND" for clean reviews and otherwise
/// list findings with file:line refs. Older protocols used REVIEW_PASS
/// / REVIEW_FAIL — we still recognise those so users can override the
/// prompt with a stricter contract.
///
/// Returns:
///   Some(true)  – clean ("NO ISSUES FOUND" or REVIEW_PASS)
///   Some(false) – explicit failure (REVIEW_FAIL or critical/major
///                 findings detected by `transcript_has_critical_issues`)
///   None        – ambiguous; caller decides (we currently treat None as
///                 pass to avoid blocking on reviewers that respond
///                 narratively without any of these markers).
fn external_review_decision(transcript: &str) -> Option<bool> {
    let upper = transcript.to_ascii_uppercase();
    if upper.contains("REVIEW_FAIL") {
        return Some(false);
    }
    if upper.contains("REVIEW_PASS") {
        return Some(true);
    }
    if upper.contains("NO ISSUES FOUND")
        || upper.contains("NO ISSUES.")
        || upper.contains("NO ISSUES\n")
    {
        return Some(true);
    }
    if transcript_has_critical_issues(transcript) {
        return Some(false);
    }
    // No explicit marker. Treat as pass — the reviewer ran, exited 0,
    // and didn't surface critical findings.
    Some(true)
}

fn transcript_has_critical_issues(transcript: &str) -> bool {
    // Only treat findings-style markers as critical, never the bare word.
    // Common shapes claude/codex actually emit when reporting an issue:
    //   - "Severity: critical"
    //   - "[CRITICAL]" / "**CRITICAL**" / "Critical:" at the start of a line
    //   - "CRITICAL ISSUE" / "CRITICAL FINDING" / "CRITICAL BUG"
    // We deliberately do NOT match the bare word "critical" anywhere in
    // the transcript because review prompts use it for procedural framing
    // (e.g. "CRITICAL: Do NOT proceed to Step 3 until ...") and the agent
    // tends to quote that phrasing back even when the underlying review
    // finds no problems.
    transcript.lines().any(line_flags_issue)
}

fn line_flags_issue(line: &str) -> bool {
    let trimmed = line.trim();
    let upper = trimmed.to_ascii_uppercase();
    if upper.starts_with("SEVERITY:") {
        return upper.contains("CRITICAL") || upper.contains("MAJOR");
    }
    for marker in [
        "[CRITICAL]",
        "[MAJOR]",
        "**CRITICAL**",
        "**MAJOR**",
        "CRITICAL ISSUE",
        "CRITICAL BUG",
        "CRITICAL FINDING",
        "MAJOR ISSUE",
        "MAJOR BUG",
        "MAJOR FINDING",
    ] {
        if upper.contains(marker) {
            return true;
        }
    }
    // "Critical:" / "Major:" at the start of a line (common bullet style).
    if upper.starts_with("CRITICAL:") || upper.starts_with("MAJOR:") {
        return true;
    }
    false
}

fn first_line_of_findings(transcript: &str) -> String {
    transcript
        .lines()
        .find(|line| line_flags_issue(line))
        .unwrap_or("")
        .trim()
        .to_string()
}

pub struct ExternalReviewArgs<'a> {
    pub prompts: &'a Prompts,
    pub implementer_command: &'a str,
    pub reviewer_command: &'a str,
    pub plan_path: &'a std::path::Path,
    pub progress_path: &'a std::path::Path,
    pub default_branch: &'a str,
    pub agent_timeout: Duration,
    pub max_iterations: usize,
}

pub async fn external_review(args: ExternalReviewArgs<'_>) -> Result<ReviewOutcome> {
    let plan_str = args.plan_path.to_string_lossy().to_string();
    let progress_str = args.progress_path.to_string_lossy().to_string();
    let default_branch = args.default_branch.to_string();
    let repo_root: PathBuf = std::env::current_dir()?;

    let mut last_category: Option<String> = None;
    let mut consecutive = 0usize;

    let spinner = crate::spinner::Spinner::start(format!(
        "phase 2 external review: spawning reviewer (1/{})",
        args.max_iterations
    ));

    for iteration in 1..=args.max_iterations {
        if let Some(s) = spinner.as_ref() {
            s.set_label(format!(
                "phase 2 external review: reviewing ({iteration}/{})",
                args.max_iterations
            ));
        }

        let mut vars: HashMap<&str, &str> = HashMap::new();
        vars.insert("PLAN_FILE", &plan_str);
        vars.insert("PROGRESS_FILE", &progress_str);
        vars.insert("DEFAULT_BRANCH", &default_branch);

        let review_prompt = substitute(&args.prompts.codex_review, &vars);
        let bumper: Option<crate::agent_driver::EventSink> = spinner.as_ref().map(|s| {
            let s = s.clone();
            let sink: crate::agent_driver::EventSink = Arc::new(move |_ev| s.bump_activity());
            sink
        });
        let review_run = crate::agent_driver::drive_agent(crate::agent_driver::AgentSpec {
            command: args.reviewer_command,
            task_prompt: &review_prompt,
            repo_root: &repo_root,
            idle_timeout: args.agent_timeout,
            cancel: None,
            event_sink: bumper,
        })
        .await?;
        if review_run.timed_out {
            anyhow::bail!("external reviewer iteration {iteration} timed out");
        }

        // Prefer the captured BEGIN..END slice; fall back to the
        // ANSI-prompt-stripped full transcript so legacy reviewers still
        // surface their verdict.
        let agent_transcript = review_run.captured_response.clone().unwrap_or_else(|| {
            crate::runner::transcript_without_prompt_echo(&review_run.transcript, &review_prompt)
        });
        let decision = external_review_decision(&agent_transcript);
        match decision {
            Some(true) => {
                if let Some(s) = spinner.as_ref() {
                    s.stop();
                }
                drop(spinner);
                return Ok(ReviewOutcome::Pass);
            }
            Some(false) => {
                let category = crate::runner::review_failure_category(&review_run.transcript);
                if last_category.as_deref() == Some(category.as_str()) {
                    consecutive += 1;
                } else {
                    consecutive = 1;
                    last_category = Some(category.clone());
                }
                if consecutive >= 3 {
                    if let Some(s) = spinner.as_ref() {
                        s.stop();
                    }
                    drop(spinner);
                    return Ok(ReviewOutcome::Issues(vec![format!(
                        "external review stalemate: category '{category}' repeated {consecutive} times"
                    )]));
                }
                if let Some(s) = spinner.as_ref() {
                    s.set_label(format!(
                        "phase 2 external review: reviewer flagged {category}, dispatching fixer ({iteration}/{})",
                        args.max_iterations
                    ));
                }
                let findings = format!("Previous external review failed: {category}");
                let mut fix_vars = vars.clone();
                fix_vars.insert("REVIEW_FINDINGS", &findings);
                let fix_prompt = substitute(&args.prompts.codex, &fix_vars);
                let fix_bumper: Option<crate::agent_driver::EventSink> =
                    spinner.as_ref().map(|s| {
                        let s = s.clone();
                        let sink: crate::agent_driver::EventSink =
                            Arc::new(move |_ev| s.bump_activity());
                        sink
                    });
                let fix_run = crate::agent_driver::drive_agent(crate::agent_driver::AgentSpec {
                    command: args.implementer_command,
                    task_prompt: &fix_prompt,
                    repo_root: &repo_root,
                    idle_timeout: args.agent_timeout,
                    cancel: None,
                    event_sink: fix_bumper,
                })
                .await?;
                if fix_run.timed_out {
                    anyhow::bail!("external review fixer iteration {iteration} timed out");
                }
            }
            None => {
                anyhow::bail!(
                    "external review iteration {iteration} did not emit REVIEW_PASS or REVIEW_FAIL"
                );
            }
        }
    }

    if let Some(s) = spinner.as_ref() {
        s.stop();
    }
    drop(spinner);

    Ok(ReviewOutcome::Issues(vec![format!(
        "external review exhausted {} iterations without REVIEW_PASS",
        args.max_iterations
    )]))
}
