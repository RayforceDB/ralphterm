# Ralphex Execution-Model Rewrite — Design Spec

**Date:** 2026-05-17
**Goal:** Rewrite RalphTerm's core runner around ralphex's execution model so `ralphterm <plan>` produces observably equivalent behavior to `ralphex <plan>` against the same plan, agent, and reviewer.

## Why this exists

The current runner sends one prompt per task (~4 lines: "do this, print COMPLETED") and marks checkboxes itself. Ralphex sends one substantial prompt (~200 lines) per iteration that gives the agent the whole plan and instructs it to navigate, pick a task, implement, and mark its own checkbox. This is a fundamentally different execution model. Matching flag names without matching the model means we are not a drop-in. Side-by-side runs of `hello.md` through both binaries showed:

- ralphex: refuses dirty worktree, auto-creates `<plan-slug>` branch, runs full 3-phase review pipeline (5 parallel reviewer agents → codex external review → 2 parallel reviewer agents), prints timestamped agent narration with iteration headers, auto-moves plan to `docs/plans/completed/`, prints `completed in Xs (Y files, +A/-B lines)` summary.
- ralphterm v0.1.2: none of the above.

## Decisions captured during brainstorming

| Decision | Choice |
|---|---|
| Prompt source | Copy ralphex prompts (`task.txt`, `make_plan.txt`, `review_first.txt`, `review_second.txt`, `codex.txt`, `codex_review.txt`, `custom_eval.txt`, `custom_review.txt`, `finalize.txt`, agent files) verbatim from `umputun/ralphex` MIT-licensed source. **No attribution file in our repo per user instruction**, despite MIT requiring it (noted as the user's call). |
| Prompt storage | Embedded as `const &str` directly in `src/prompts.rs` — no `.txt` files in source tree. Loader still honors `.ralphex/prompts/<name>.txt` and `~/.config/ralphex/prompts/<name>.txt` overrides. |
| Review-phase scope | Full 3-phase parity: 5-agent first review (quality, implementation, testing, simplification, documentation), codex external review with fixer loop, 2-agent second review. |
| Verification | After every implementation task, run the same `hello.md` plan through both `ralphex` (1.2.0) and `ralphterm` from a clean state, diff the transcripts. Plan task is "done" only when the diff shows no structural divergence. |

---

## Architecture

### Module layout

| Path | Responsibility |
|---|---|
| `src/prompts.rs` | NEW. Embedded prompt constants + agent definition constants. `Prompts::load(project_root)` returns a `Prompts` struct with `task`, `review_first`, `review_second`, `codex`, `codex_review`, `custom_eval`, `custom_review`, `finalize`, `make_plan`, and `Vec<AgentDef>` fields. Each accessor reads `.ralphex/prompts/<name>.txt`, then `~/.config/ralphex/prompts/<name>.txt`, then falls back to the embedded const. Also exposes `substitute(template, vars: &HashMap<&str, &str>) -> String` for `{{VAR}}` replacement. |
| `src/preflight.rs` | NEW. `Preflight::check(repo_root, plan_path, use_worktree, branch_override) -> PreflightResult`. Refuses dirty worktree unless `--worktree` is set, derives plan slug, creates branch `<slug>` (or `<override>`), produces the same error messages ralphex prints. |
| `src/review_phases.rs` | NEW. Three entry points: `first_review(prompts, agents, run_state) -> ReviewOutcome`, `external_review(...)`, `second_review(...)`. `ReviewOutcome` is `Pass`, `Issues(Vec<Finding>)`, or `Stalemate`. Phase 1 and Phase 3 dispatch their reviewer-agent calls concurrently via `tokio::join!`. |
| `src/output_format.rs` | NEW. Thin formatter that prints ralphex-style stdout strings: `creating branch: <slug>`, `starting ralphex loop (max N iterations) (<mode>)`, `plan: <path>`, `branch: <name>`, `progress log: <path>`, blank line, `starting task execution phase`, blank line, `--- task iteration N ---`, eventually `completed in Xs (Y files, +A/-B lines)` and the indented summary footer. |
| `src/progress_log.rs` | NEW. Writes `.ralphex/progress/progress-<slug>.txt` in ralphex's format: timestamped `[YYYY-MM-DD HH:MM:SS]` agent narration lines + control lines (`creating branch`, `--- task iteration N ---`, etc.). Replaces the existing `.ralphterm/progress/<slug>.log` `key=value` format. |
| `src/runner.rs` | REWRITTEN. Down to a thin orchestrator that calls preflight, then the new iteration loop (`task_execution_phase`), then `review_phases::first_review` / `external_review` / `second_review` (skipped per mode flag), then `finalize`. Estimated final size: ~600 lines (down from 2700). |
| `src/runs.rs` | UPDATED. Run records gain a `phase: TaskPhase | FirstReviewPhase | ExternalReviewPhase | SecondReviewPhase | FinalizingPhase` enum to match the new phase set. |
| `src/cli.rs` | UPDATED. Mode flags rewired to the new pipeline; output strings (success summary, etc.) emitted via `output_format`. |

### Iteration loop pseudocode

```
fn task_execution_phase(prompts, plan, progress, agent_cmd, max_iterations) -> Result<()> {
    for iteration in 1..=max_iterations {
        if all_checkboxes_complete(plan)? { return Ok(()); }
        write_line(progress, "--- task iteration {iteration} ---");
        let vars = HashMap::from([
            ("PLAN_FILE", plan.to_str()?),
            ("PROGRESS_FILE", progress.to_str()?),
            ("GOAL", goal_from_plan(plan)?),
            ("DEFAULT_BRANCH", default_branch()?),
        ]);
        let prompt = prompts.substitute(&prompts.task, &vars);
        let run = run_agent(agent_cmd, &prompt, agent_timeout)?;
        for line in extract_agent_narration(&run.transcript) {
            write_timestamped(progress, &line);
            println_passthrough(&line);
        }
        if run.transcript.contains("<<<RALPHEX:ALL_TASKS_DONE>>>") { return Ok(()); }
    }
    bail!("hit max iterations ({max_iterations}) without ALL_TASKS_DONE");
}
```

`extract_agent_narration` parses the agent's stdout looking for the `STEP 0 - ANNOUNCE` block (per ralphex's `task.txt` instructions, the agent emits a brief overview) and any other timestamped lines, ignoring TUI noise / ANSI control sequences.

### Phase 1 first review (5 agents in parallel)

`review_first.txt` is the meta-prompt; each of the 5 agent files (`quality.txt`, `implementation.txt`, `testing.txt`, `simplification.txt`, `documentation.txt`) is a role specialization. We send 5 prompts in parallel through `tokio::task::spawn_blocking` (each wraps `run_agent_command_with_timeout`). Each agent is asked to report findings as JSON (per ralphex's prompt template). We parse, collect, and:

- Zero critical/major findings across all 5 → Pass.
- Any critical → feed findings back to the implementer via a fixup task (re-run `task_execution_phase` with the findings injected into the prompt context), loop bounded by `max_iterations`.

### Phase 2 external review

Already partially exists as `run_plan_external_only`. Refactored into `external_review`. Same `--review-patience` / `--max-external-iterations` semantics. Driven by `codex_review.txt`.

### Phase 3 second review (2 agents in parallel)

Same shape as Phase 1 but only 2 of the agent files (per ralphex's behavior — `review_second.txt` selects which). Tighter pass criterion: only critical/major matter; minor findings are advisory.

### Finalize

Runs `finalize.txt` prompt against the implementer. Always moves plan to `<plan-dir>/completed/<filename>` unless `--no-move-completed`. Prints summary footer:

```
completed in {N}s ({F} files, +{A}/-{D} lines)
  plan: {dest-path}
  branch: {branch-name}
  progress log: {progress-path}
```

Where `F/A/D` come from `git diff --shortstat <base-ref>..HEAD`. `N` is wallclock seconds from `creating branch` to summary print.

---

## Vendored prompt content

All 9 prompts + 5 agent files copied verbatim from `umputun/ralphex` v1.2.0 (`pkg/config/defaults/prompts/` and `pkg/config/defaults/agents/`). Total embedded size: ~50 KB. Stored as `pub(crate) const FOO_TXT: &str = r#"..."#;` in `src/prompts.rs`. The single file balloons to ~60 KB of source; acceptable per the user's "minimize files bloat" preference.

Variables ralphex substitutes (we match): `{{PLAN_FILE}}`, `{{PROGRESS_FILE}}`, `{{GOAL}}`, `{{DEFAULT_BRANCH}}`, `{{TRANSCRIPT_FILE}}`, `{{REVIEW_OUTPUT_FILE}}`.

---

## Run flow (full mode)

```
ralphterm <plan>
  ↓
preflight: refuse dirty worktree → create branch <slug>
  ↓
output_format: "creating branch: <slug>" → "starting ralphex loop..." → "starting task execution phase"
  ↓
task_execution_phase: iterate, agent picks one task per iteration, marks own checkbox
  until <<<RALPHEX:ALL_TASKS_DONE>>> or max-iterations
  ↓
output_format: "all tasks completed, starting code review..."
  ↓
review_phases::first_review (5 parallel reviewers via review_first.txt + agent/*.txt)
  if findings → loop back to task_execution_phase with findings injected
  ↓
review_phases::external_review (codex via codex_review.txt + --review-patience loop)
  if changes → fixup → re-review
  ↓
review_phases::second_review (2 parallel reviewers via review_second.txt)
  if critical findings → fixup → re-review
  ↓
finalize: run finalize.txt, move plan to completed/, print summary
```

Mode flags reroute:
- `--tasks-only`: preflight → task_execution_phase → finalize (skip all reviews; no plan move unless `--move-completed`).
- `--review`: skip preflight (assume worktree is in expected state) → first_review → external_review → second_review → finalize.
- `--external-only` / `--codex-only`: skip preflight → external_review → finalize.

---

## CLI surface

No new flags. The existing surface (already 90% of ralphex's flag names) is re-wired to the new pipeline. Documented divergences after this rewrite:

- `--idle-timeout` still accepted-but-no-op (no PTY idle detection).
- `--wait` still accepted-but-no-op (no rate-limit-aware retry).
- `--init`, `--reset`, `--dump-defaults` still not implemented (deferred).
- `-V` is short for `--version` (clap convention); ralphex uses `-v`.

Everything else should match ralphex behavior by the end of this work.

---

## Verification

After every implementation task, run this script from a clean checkout of both binaries:

```sh
#!/bin/sh
set -eu
TMP=$(mktemp -d)
cd "$TMP"
git init -q && git config user.email t@e.invalid && git config user.name test
mkdir -p docs/plans
cat > docs/plans/hello.md <<'PLAN'
# Hello plan

## Validation Commands
- `test -f hello.txt`

### Task 1: write the file
- [ ] Create a file named hello.txt with the text "hi"
PLAN
git add -A && git commit -q -m init
/tmp/ralphex-bin/ralphex --tasks-only docs/plans/hello.md > /tmp/ralphex.out 2>&1
git reset --hard HEAD -q && git clean -fdq
git checkout -q master 2>/dev/null || git checkout -q main 2>/dev/null || true
git branch -D hello 2>/dev/null || true
/path/to/ralphterm --tasks-only docs/plans/hello.md > /tmp/ralphterm.out 2>&1
diff -u /tmp/ralphex.out /tmp/ralphterm.out | head -80
```

The diff must show only acceptable variation: timestamp differences, ralphterm version banner instead of ralphex's, hash differences. Structural differences (missing lines, wrong order, different headers, different summary format) are blockers — fix before the next task starts.

Beyond `hello.md`, two more plans get verified after the Phase 1 task lands:

1. `multi-task.md` — two checkboxes; verifies iteration counter and inter-task continuation.
2. `with-review.md` — one task plus review pipeline through Phase 3 (using a real `codex` install).

Test fixtures get rewritten in lockstep: the new `fake-agent.sh` reads the prompt, opens `{{PLAN_FILE}}`, finds the first `- [ ]`, performs the recipe (file write), replaces with `- [x]`, prints `<<<RALPHEX:ALL_TASKS_DONE>>>` when all boxes are checked. Existing tests that assert the old per-task output strings are rewritten to assert the new ralphex-style strings.

---

## Out of scope

Deferred to a follow-up release:

- `--init`, `--reset`, `--dump-defaults` — ralphex's config bootstrap commands.
- Web dashboard parity beyond what already ships in `dashboard/`.
- Notifications per-phase event matching (we currently fire on plan-done / task-failed / review-failed / rate-limit; ralphex emits more events).
- Per-iteration JSON event stream alongside the text progress log.
- Replicating ralphex's `--reset` interactive flow.
- Matching ralphex's exact exit code mapping for every error class.

---

## Acceptance

The rewrite is done when:

1. The verification script above shows no structural diff between `ralphex --tasks-only hello.md` and `ralphterm --tasks-only hello.md` against `hello.md`, `multi-task.md`, and `with-review.md`.
2. `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all` all pass.
3. `src/runner.rs` is under 1000 lines (it gains modular structure, loses the per-task scaffolding it doesn't need anymore).
4. README and `docs/migrate-from-ralphex.md` describe the new model honestly: drop-in for ralphex's `--tasks-only` and 3-phase review pipeline, with the deferred-features list above clearly called out.
5. v0.2.0 published to crates.io after the verification gates pass on the author's machine, not just CI.
