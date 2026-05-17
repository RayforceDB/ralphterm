//! Embedded ralphex 1.2.0 prompt and agent texts.
//!
//! These constants are the default templates used when the project (or
//! global) override directories do not provide their own `.txt` copies.

#![allow(dead_code)]
pub(crate) const TASK_TXT: &str = r####"# task execution prompt
# this prompt is used for each task iteration in phase 1
#
# available variables:
#   {{PLAN_FILE}} - path to the plan file being executed
#   {{PROGRESS_FILE}} - path to the progress log file
#   {{GOAL}} - human-readable goal description
#   {{DEFAULT_BRANCH}} - default branch name (main, master, trunk, etc.)

# Read the plan file at {{PLAN_FILE}}. Find the FIRST Task section (### Task N: or ### Iteration N:) that has uncompleted checkboxes ([ ]).

# If NO Task section has [ ] but ## Success criteria, ## Overview, or ## Context still has [ ]: either satisfy those items and mark them [x] if actionable, or output <<<RALPHEX:ALL_TASKS_DONE>>> if they are verification-only (manual testing, deployment, etc.) — do not loop indefinitely when remaining items are not actionable by you.

# If a Task section has [ ] checkboxes you cannot complete (manual testing, deployment verification, external checks): mark them [x] with a note like "[x] manual test (skipped - not automatable)" and proceed. Do not loop indefinitely on non-automatable items inside Task sections.

# NOTE: Progress is logged to {{PROGRESS_FILE}} - this file contains detailed execution steps and can be reviewed for debugging.

# CRITICAL CONSTRAINT: Complete ONE Task section per iteration.
# A Task section is a "### Task N:" or "### Iteration N:" header with all its checkboxes underneath.
# Complete ALL checkboxes in that section, then STOP.
# Do NOT continue to the next section - the external loop will call you again for it.

# STEP 0 - ANNOUNCE:
# Before starting work, output a brief overview (up to 200 words) explaining:
# - Which task number you picked and its title
# - What the task will accomplish
# - Key files or components involved
# This helps the user understand what's happening in the current iteration.

# STEP 1 - IMPLEMENT:
# - Read the plan's Overview and Context sections to understand the work
# - Implement ALL items in the current Task section (all [ ] checkboxes under it)
# - Write tests for the implementation

# STEP 2 - VALIDATE:
# - Run the test and lint commands specified in the plan (e.g., "cargo test", "go test ./...", etc.)
# - Fix any failures, repeat until all validation passes

# STEP 3 - COMPLETE (after validation passes):
# - Update progress: edit {{PLAN_FILE}} and change [ ] to [x] for each checkbox you implemented in the current Task section. If Task sections are complete but ## Success criteria, ## Overview, or ## Context has [ ] items that the implementation satisfies, mark them [x] in this same edit to avoid extra loop iterations. If any such items are NOT satisfied, do NOT mark them and do NOT output ALL_TASKS_DONE — continue to the next iteration to address them.
# - Commit all changes (code + updated plan) with message: feat: <brief task description>
# - Check if any [ ] checkboxes remain in Task sections (### Task N: or ### Iteration N:)
# - If NO more [ ] checkboxes in the entire plan, output exactly: <<<RALPHEX:ALL_TASKS_DONE>>>
# - If more Task sections have [ ] checkboxes, STOP HERE - do not continue

# If any phase fails after reasonable fix attempts, output exactly: <<<RALPHEX:TASK_FAILED>>>

# REMINDER: ONE section (Task/Iteration) per loop cycle. After commit, STOP and let the loop handle the next section.

# OUTPUT FORMAT: No markdown formatting (no **bold**, `code`, # headers). Plain text and - lists are fine. Do not echo phase names or step numbers - just do the work.
"####;

pub(crate) const MAKE_PLAN_TXT: &str = r#"# plan creation prompt
# this prompt is used for interactive plan creation mode
# claude explores the codebase, asks clarifying questions, and creates a plan
#
# available variables:
#   {{PLAN_DESCRIPTION}} - user's original request for what to implement
#   {{PROGRESS_FILE}} - path to progress file with Q&A history
#   {{DEFAULT_BRANCH}} - default branch name (main, master, trunk, etc.)
#   {{PLANS_DIR}} - plans directory (default: docs/plans)

# You are helping create an implementation plan for: {{PLAN_DESCRIPTION}}

# Progress log: {{PROGRESS_FILE}} (contains previous Q&A from this session)

# IMPORTANT: Read the progress file first to see any questions you already asked and answers provided. Do not repeat questions.

## Step 0: Check for Existing Plan

# FIRST, check if a plan file already exists in {{PLANS_DIR}}/ matching this request.
# If a plan file for this feature already exists:
# - Output <<<RALPHEX:PLAN_READY>>> immediately
# - Do NOT modify the existing plan
# - STOP - do not output anything else

## Step 1: Read Progress File

# Read {{PROGRESS_FILE}} to understand:
# - What questions you have already asked
# - What answers the user provided
# - Any exploration notes from previous iterations

## Step 2: Explore the Codebase

# If this is your first iteration (no Q&A in progress file):
# - Search for relevant files and patterns
# - Understand the project structure
# - Identify existing conventions and patterns
# - Find related code that will inform the implementation

## Step 3: Ask Clarifying Questions (if needed)

# If you need user input to create a good plan, emit a QUESTION signal:

# <<<RALPHEX:QUESTION>>>
# {"question": "Your question here?", "options": ["Option 1", "Option 2", "Option 3"]}
# <<<RALPHEX:END>>>

# Rules for questions:
# - Ask ONE question at a time
# - Provide 2-4 concrete options (not vague like "other")
# - Only ask if you genuinely need clarification
# - Do not ask about implementation details you can decide yourself
# - Focus on architectural choices, feature scope, and user preferences

# After emitting QUESTION, STOP immediately. Do not continue. The loop will collect the answer and run another iteration.

## Step 3.5: Present Draft for Review

# When you have enough information to create a plan, present it as a draft for user review BEFORE writing to disk.

# Emit the plan draft:

# <<<RALPHEX:PLAN_DRAFT>>>
# <Title>

## Overview
# <Brief description of what will be implemented>

## Context
# - Files involved: <list relevant files>
# ...

## Implementation Steps
# ...
# <<<RALPHEX:END>>>

# CRITICAL: After emitting PLAN_DRAFT, STOP immediately. Do not continue. Do not write the plan file yet.

# The loop will:
# 1. Display the draft to the user with terminal rendering
# 2. Ask the user to Accept, Revise, Interactive review (open in $EDITOR), or Reject
# 3. Run another iteration with the user's decision

# **Handling user responses:**

# If user ACCEPTS (progress file contains "DRAFT REVIEW: accept"):
# - Proceed to Step 4 to write the plan file to disk
# - Then emit PLAN_READY

# If user requests REVISION (progress file contains "DRAFT REVIEW: revise" and "FEEDBACK:"):
# - Read the feedback from the progress file
# - Feedback may be free-form text (from "Revise") or a unified diff with interpretation instructions (from "Interactive review" where the user edited the plan in $EDITOR). Both formats indicate what the user wants changed — apply the requested modifications
# - Modify the plan based on the feedback
# - Emit a new PLAN_DRAFT with the updated plan
# - STOP and wait for next review

# If user REJECTS (progress file contains "DRAFT REVIEW: reject"):
# - Output exactly: <<<RALPHEX:TASK_FAILED>>>
# - STOP immediately - the user has cancelled plan creation

## Step 4: Write Plan File (after draft accepted)

# This step executes ONLY after the user accepts your draft (progress file contains "DRAFT REVIEW: accept").

# Write the accepted plan to disk:

# 1. Create a plan file at {{PLANS_DIR}}/YYYY-MM-DD-<slug>.md where <slug> is derived from the description
# 2. Use this structure:

# ---
# <Title>

## Overview
# <Brief description of what will be implemented>

## Context
# - Files involved: <list relevant files>
# - Related patterns: <existing patterns to follow>
# - Dependencies: <external dependencies if any>

## Development Approach
# - **Testing approach**: Regular (code first, then tests) or TDD (test first)
# - Complete each task fully before moving to the next
# - <Any project-specific approaches>
# - **CRITICAL: every task MUST include new/updated tests**
# - **CRITICAL: all tests must pass before starting next task**

## Implementation Steps

### Task 1: <Title>

# **Files:**
# - Modify: `path/to/file`
# - Create: `path/to/new_file` (if any)

# - [ ] first implementation step
# - [ ] second implementation step
# - [ ] write tests for this task
# - [ ] run project test suite - must pass before task 2

### Task 2: <Title>
# ...

# (continue for all tasks)

### Task N: Verify acceptance criteria

# - [ ] run full test suite (use project-specific command)
# - [ ] run linter (use project-specific command)
# - [ ] verify test coverage meets 80%+

### Task N+1: Update documentation

# - [ ] update README.md if user-facing changes
# - [ ] update CLAUDE.md if internal patterns changed
# ---

## Step 4.5: Validate Plan Before Draft

# Before emitting PLAN_DRAFT in Step 3.5, verify the plan against these criteria:

# **Scope & Feasibility:**
# - [ ] Tasks are reasonably sized (aim for 3-7 items; adjust if needed for coherence)
# - [ ] Each task focuses on one component or closely related files
# - [ ] Task dependencies are linear (no circular deps)
# - [ ] External dependencies are minimized and clearly noted

# **Completeness:**
# - [ ] All requirements from the original description are addressed
# - [ ] Each task specifies file paths where known (use patterns for discovery tasks)
# - [ ] Each task that modifies code includes test items
# - [ ] Task section checkboxes are automatable by the agent (no manual testing, deployment, or external verification items as `- [ ]` inside Task sections; those go in Post-Completion)

# **Simplicity (YAGNI):**
# - [ ] No unnecessary abstractions
# - [ ] No "future-proofing" features not in the original request
# - [ ] No backwards compatibility or fallbacks unless explicitly requested
# - [ ] New files only for genuinely new components, not minor additions
# - [ ] No over-engineered patterns when simpler solutions work

# If validation fails, fix the plan before emitting PLAN_DRAFT.

# Only after validation passes:
# 1. Emit PLAN_DRAFT (Step 3.5) and wait for user review
# 2. If user accepts, write the plan file (Step 4)
# 3. After writing the file, emit PLAN_READY:
#    - Output exactly: <<<RALPHEX:PLAN_READY>>>
#    - STOP IMMEDIATELY - do not output anything else after this signal

# CRITICAL RULES:
# - DO NOT ask "Would you like to proceed?" or "Should I implement this?" or similar
# - DO NOT wait for user approval - ralphex handles confirmation externally
# - DO NOT use natural language questions - only use <<<RALPHEX:QUESTION>>> signal format
# - DO NOT iterate or refine the plan after validation passes
# - DO NOT translate the `### Task N:` and `### Iteration N:` section headers. These are structural tokens required by ralphex's parser and MUST use those exact English keywords even when the plan content is written in another language (Russian, Chinese, Spanish, etc.). Task titles and body text may be in the requested language; only the `Task` / `Iteration` keyword and the numbered format are fixed.
# - The PLAN_READY signal means "plan is complete, session is done"

# OUTPUT FORMAT: No markdown formatting in your response text (no **bold**, `code`, # headers). Plain text and - lists are fine. The plan FILE should use markdown.
"#;

pub(crate) const REVIEW_FIRST_TXT: &str = r#"# first review prompt
# this prompt is used for the first (comprehensive) review pass in phase 2
# launches 5 parallel agents for thorough code review
#
# available variables:
#   {{PLAN_FILE}} - path to the plan file being executed
#   {{PROGRESS_FILE}} - path to the progress log (task execution + previous reviews)
#   {{GOAL}} - human-readable goal description
#   {{DEFAULT_BRANCH}} - default branch name (main, master, trunk, etc.)
#   {{agent:<name>}} - expands to Task tool instructions for the named agent
#
# agents are defined in ~/.config/ralphex/agents/ (user) or pkg/config/defaults/agents/ (builtin)

# Code review of: {{GOAL}}

# Progress log: {{PROGRESS_FILE}} (contains task execution and previous review iterations)

## Step 1: Get Branch Context

# Run both commands to understand what was done:
# - `git log {{DEFAULT_BRANCH}}..HEAD --oneline` - see commit history (what was implemented)
# - `git diff {{DEFAULT_BRANCH}}...HEAD` - see actual code changes

## Step 2: Launch ALL 5 Review Agents IN PARALLEL

# All Task tool calls MUST be in the same message for parallel foreground execution.
# Do NOT use run_in_background. Foreground agents run in parallel and block until all complete — no TaskOutput polling needed.

# CRITICAL: Do NOT proceed to Step 3 until ALL 5 agents have returned results.

# Agents to launch:
# {{agent:quality}}
# {{agent:implementation}}
# {{agent:testing}}
# {{agent:simplification}}
# {{agent:documentation}}

# Each agent prompt should be short — do NOT paste the diff into it. Instead, instruct each agent to:
# 1. Run `git diff {{DEFAULT_BRANCH}}...HEAD` and `git diff --stat {{DEFAULT_BRANCH}}...HEAD` to get the changes
# 2. Read the actual source files to review code in full context
# 3. Report problems only - no positive observations

## Step 3: Collect, Verify, and Fix Findings

# After agents complete:

### 3.1 Collect and Deduplicate
# - Merge findings from all agents
# - Same file:line + same issue → merge
# - Cross-agent duplicates → merge, note both sources

### 3.2 Verify EVERY Finding (CRITICAL)
# For EACH issue (bugs, test gaps, smells, over-engineering, error handling, docs, etc.):
# 1. Read actual code at file:line
# 2. Check full context (20-30 lines around)
# 3. Verify issue is real, not a false positive
# 4. Check for existing mitigations

# Classify as:
# - CONFIRMED: Real issue, fix it
# - FALSE POSITIVE: Doesn't exist or already mitigated - discard

# IMPORTANT: Pre-existing issues (linter errors, failed tests) should also be fixed.
# Do NOT reject issues just because they existed before this branch - fix them anyway.

### 3.3 Fix All Confirmed Issues
# 1. Fix all CONFIRMED issues (all types: bugs, tests, smells, docs, etc.)
# 2. Run tests and linter to verify fixes - ALL tests must pass, ALL linter issues resolved
# 3. Commit fixes: `git commit -m "fix: address code review findings"`

## Step 4: Signal Completion

# SIGNAL LOGIC - READ CAREFULLY:

# IMPORTANT: Do not decide on a signal path until you have completed Steps 1-3 in full — all agents finished, all results collected, all findings verified and acted on.

# REVIEW_DONE means "this iteration found ZERO issues" - NOT "I finished fixing issues".

# Path A - NO confirmed issues found:
# - You reviewed the code and found nothing to fix
# - Output: <<<RALPHEX:REVIEW_DONE>>>

# Path B - Issues found AND fixed:
# - You found issues, fixed them, and committed
# - STOP HERE. Do NOT output any signal. Do NOT output REVIEW_DONE.
# - The external loop will run another review iteration to verify your fixes.
# - Your fixes might have introduced new issues - another iteration must check.

# Path C - Issues found but cannot fix:
# - Output: <<<RALPHEX:TASK_FAILED>>>

# OUTPUT FORMAT: No markdown formatting (no **bold**, `code`, # headers). Plain text and - lists are fine.
"#;

pub(crate) const REVIEW_SECOND_TXT: &str = r#"# second review prompt
# this prompt is used for the final review pass in phase 4
# focuses on critical/major issues only, uses 2 agents
#
# available variables:
#   {{PLAN_FILE}} - path to the plan file being executed
#   {{PROGRESS_FILE}} - path to the progress log (task execution + previous reviews)
#   {{GOAL}} - human-readable goal description
#   {{DEFAULT_BRANCH}} - default branch name (main, master, trunk, etc.)
#   {{agent:<name>}} - expands to Task tool instructions for the named agent
#
# agents are defined in ~/.config/ralphex/agents/ (user) or pkg/config/defaults/agents/ (builtin)

# Second code review pass of: {{GOAL}}

# Progress log: {{PROGRESS_FILE}} (contains task execution and previous review iterations)

## Step 1: Get Branch Context

# Run both commands to understand what was done:
# - `git log {{DEFAULT_BRANCH}}..HEAD --oneline` - see commit history (what was implemented)
# - `git diff {{DEFAULT_BRANCH}}...HEAD` - see actual code changes

## Step 2: Launch Review Agents IN PARALLEL

# All Task tool calls MUST be in the same message for parallel foreground execution.
# Do NOT use run_in_background. Foreground agents run in parallel and block until all complete — no TaskOutput polling needed.

# CRITICAL: Do NOT proceed to Step 3 until BOTH agents have returned results.

# Agents to launch:
# {{agent:quality}}
# {{agent:implementation}}

# Each agent prompt should be short — do NOT paste the diff into it. Instead, instruct each agent to:
# 1. Run `git diff {{DEFAULT_BRANCH}}...HEAD` and `git diff --stat {{DEFAULT_BRANCH}}...HEAD` to get the changes
# 2. Read the actual source files to review code in full context
# 3. Report problems only - no positive observations

# Focus only on critical and major issues. Ignore style/minor issues.

## Step 3: Verify and Evaluate Findings

### 3.1 Verify Each Finding
# For each issue reported:
# 1. Read actual code at file:line
# 2. Verify issue is real (not false positive)
# 3. Check if it's truly critical/major severity

### 3.2 Act on Verified Findings

# IMPORTANT: Pre-existing issues (linter errors, failed tests) should also be fixed.
# Do NOT reject issues just because they existed before this branch - fix them anyway.

# SIGNAL LOGIC - READ CAREFULLY:

# IMPORTANT: Do not decide on a signal path until you have completed Steps 1-3 in full — all agents finished, all results collected, all findings verified and acted on.

# REVIEW_DONE means "this iteration found ZERO issues" - NOT "I finished fixing issues".

# Path A - NO issues found in this iteration:
# - You reviewed the code and found nothing critical/major to fix
# - Output: <<<RALPHEX:REVIEW_DONE>>>

# Path B - Issues found AND fixed:
# 1. Fix verified critical/major issues only
# 2. Run tests and linter - ALL tests must pass, ALL linter issues resolved
# 3. Commit fixes: `git commit -m "fix: address code review findings"`
# 4. STOP HERE. Do NOT output any signal. Do NOT output REVIEW_DONE.
#    The external loop will run another review iteration to verify your fixes.
#    Your fixes might have introduced new issues - another iteration must check.

# Path C - Issues found but cannot fix:
# - Output: <<<RALPHEX:TASK_FAILED>>>

# OUTPUT FORMAT: No markdown formatting (no **bold**, `code`, # headers). Plain text and - lists are fine.
"#;

pub(crate) const CODEX_TXT: &str = r#"# codex evaluation prompt
# this prompt is used when claude evaluates codex review output
# codex runs in phase 3, between first and second claude reviews
#
# available variables:
#   {{PLAN_FILE}} - path to the plan file being executed
#   {{GOAL}} - human-readable goal description
#   {{DEFAULT_BRANCH}} - default branch name (main, master, trunk, etc.)
#   {{CODEX_OUTPUT}} - output from codex code review

# External code review evaluation.

# Codex reviewed the code and found:

# ---
# {{CODEX_OUTPUT}}
# ---

## Your Task

# Analyze each finding critically. For EACH issue:

# 1. Read the code at the reported location and trace the flow - find callers, see what functions it calls, understand the full context
# 2. Understand what the code does, why it was written this way, and how the reported issue actually affects behavior
# 3. Check {{PLAN_FILE}} - was this an intentional design decision?
# 4. Assess the actual impact - is this a real problem or a style preference?

# Then categorize:

# - **Valid issues**: Fix them (edit files, run tests/linter to verify)
# - **Invalid/irrelevant issues**: Explain why they don't apply (intentional design, already mitigated, misunderstood context) - your explanation will be passed to Codex for re-evaluation

# IMPORTANT: Pre-existing issues (linter errors, failed tests) should also be fixed.
# Do NOT reject issues just because they existed before this branch - fix them anyway.

## After Evaluation

# **If there were actionable issues to fix:**
# - Fix them, run tests/linter to verify - ALL tests must pass, ALL linter issues resolved
# - Do NOT commit yet - more codex iterations may follow
# - STOP here and DO NOT output any signal - the external loop will run codex again to verify fixes
# - NEVER output CODEX_REVIEW_DONE after fixing issues

# **If you dismissed ALL findings as invalid** (codex reported issues but none are actionable):
# - Explain why each finding is invalid
# - Do NOT commit anything
# - Do NOT output any signal
# - STOP here — the loop will re-run the external tool with your explanations for context

# **If Codex reports NO actionable issues** (empty output, "no issues found", "NO ISSUES FOUND"):
# - Run `git diff` to review ALL uncommitted changes (accumulated fixes from multiple iterations)
# - Commit all fixes with message: "fix: address codex review findings"
# - Output exactly: <<<RALPHEX:CODEX_REVIEW_DONE>>>

# CRITICAL: The CODEX_REVIEW_DONE signal means "codex found nothing to fix". Only output it when codex itself reported no issues. If you fixed anything, do NOT output the signal.

# CRITICAL: Never run codex commands yourself. The external loop handles codex execution.

# OUTPUT FORMAT: No markdown formatting (no **bold**, `code`, # headers). Plain text and - lists are fine.
"#;

pub(crate) const CODEX_REVIEW_TXT: &str = r#"# codex review prompt
# this prompt is sent to the codex external review tool (or compatible wrapper)
# codex reviews code changes and reports findings for claude to evaluate
#
# available variables:
#   {{DIFF_INSTRUCTION}} - git diff command appropriate for current iteration
#   {{PLAN_FILE}} - path to the plan file being executed
#   {{PROGRESS_FILE}} - path to progress log with previous review iterations
#   {{PREVIOUS_REVIEW_CONTEXT}} - previous review context (empty on first iteration)
#   {{DEFAULT_BRANCH}} - default branch name (main, master, trunk, etc.)
#   {{GOAL}} - human-readable goal description

# Review the code changes for: {{GOAL}}

## Get the Diff

# Run: {{DIFF_INSTRUCTION}}

## Context

# Plan: {{PLAN_FILE}}

# ---

# Check the progress log at {{PROGRESS_FILE}} for previous review iterations and findings history before reporting issues.

## Review Focus

# Analyze for:
# - Bugs and logic errors
# - Security vulnerabilities
# - Race conditions
# - Error handling gaps
# - Code quality issues

# Report findings with file:line references. If no issues found, say "NO ISSUES FOUND".

# {{PREVIOUS_REVIEW_CONTEXT}}
"#;

pub(crate) const CUSTOM_EVAL_TXT: &str = r#"# custom evaluation prompt
# this prompt is used when claude evaluates custom external review tool output
# the custom review tool runs in the external review phase
#
# available variables:
#   {{PLAN_FILE}} - path to the plan file being executed
#   {{GOAL}} - human-readable goal description
#   {{DEFAULT_BRANCH}} - default branch name (main, master, trunk, etc.)
#   {{CUSTOM_OUTPUT}} - output from the custom review tool

# External code review evaluation.

# A custom review tool analyzed the code and found:

# ---
# {{CUSTOM_OUTPUT}}
# ---

## Your Task

# Analyze each finding critically. For EACH issue:

# 1. Read the code at the reported location and trace the flow - find callers, see what functions it calls, understand the full context
# 2. Understand what the code does, why it was written this way, and how the reported issue actually affects behavior
# 3. Check {{PLAN_FILE}} - was this an intentional design decision?
# 4. Assess the actual impact - is this a real problem or a style preference?

# Then categorize:

# - **Valid issues**: Fix them (edit files, run tests/linter to verify)
# - **Invalid/irrelevant issues**: Explain why they don't apply (intentional design, already mitigated, misunderstood context) - your explanation will be passed to the review tool for re-evaluation

# IMPORTANT: Pre-existing issues (linter errors, failed tests) should also be fixed.
# Do NOT reject issues just because they existed before this branch - fix them anyway.

## After Evaluation

# **If there were actionable issues to fix:**
# - Fix them, run tests/linter to verify - ALL tests must pass, ALL linter issues resolved
# - Do NOT commit yet - more review iterations may follow
# - STOP and let the external loop run the review tool again

# **If you dismissed ALL findings as invalid** (review tool reported issues but none are actionable):
# - Explain why each finding is invalid
# - Do NOT commit anything
# - Do NOT output any signal
# - STOP here — the loop will re-run the external tool with your explanations for context

# **If the review tool reports NO actionable issues** (empty output, "no issues found", "NO ISSUES FOUND"):
# - Run `git diff` to review ALL uncommitted changes (accumulated fixes from multiple iterations)
# - Commit all fixes with message: "fix: address external review findings"
# - Output exactly: <<<RALPHEX:CODEX_REVIEW_DONE>>>

# CRITICAL: Never run the external review tool yourself. The external loop handles tool execution.

# OUTPUT FORMAT: No markdown formatting (no **bold**, `code`, # headers). Plain text and - lists are fine.
"#;

pub(crate) const CUSTOM_REVIEW_TXT: &str = r#"# custom review prompt
# this prompt is sent to a custom external review tool (script)
# the script receives this as a file and should run the code review
#
# available variables:
#   {{DIFF_INSTRUCTION}} - git diff command appropriate for current iteration
#   {{GOAL}} - human-readable goal description
#   {{PLAN_FILE}} - path to the plan file being executed
#   {{PROGRESS_FILE}} - path to progress log with previous review iterations
#   {{PREVIOUS_REVIEW_CONTEXT}} - previous review context (empty on first iteration)
#   {{DEFAULT_BRANCH}} - default branch name (main, master, trunk, etc.)

# You are reviewing code changes for: {{GOAL}}

## Get the Diff

# Run this command to see the changes:
# {{DIFF_INSTRUCTION}}

## Review Focus

# Analyze the code for:

# 1. Bugs and logic errors - incorrect behavior, edge cases, null/nil handling
# 2. Security issues - injection, XSS, secrets exposure, improper validation
# 3. Race conditions - concurrent access, shared state, missing synchronization
# 4. Error handling - silent failures, ignored errors, missing fallbacks
# 5. Test coverage - missing tests, inadequate edge case coverage
# 6. Code quality - unnecessary complexity, poor naming, unclear logic

## Output Format

# Report issues as a structured list:

# - file:line - description of issue
# - file:line - description of issue

# If no issues found, output: NO ISSUES FOUND

## Previous Review History

# Check the progress log at {{PROGRESS_FILE}} for previous review iterations and findings history before reporting issues.

## Important

# - Focus on real problems, not style preferences
# - Check if issues are already mitigated in the code
# - Only report issues you can verify by reading the actual code
# - Be specific about file paths and line numbers

# {{PREVIOUS_REVIEW_CONTEXT}}
"#;

pub(crate) const FINALIZE_TXT: &str = r#"# finalize prompt
# this prompt runs once after successful reviews, best-effort (failures logged but don't block)
# disabled by default (finalize_enabled = false)
#
# available variables:
#   {{DEFAULT_BRANCH}} - default branch name (main, master, trunk, etc.)

# Post-completion finalize step.

# Rebase your commits onto the latest {{DEFAULT_BRANCH}} and organize them for merge.

# Steps:

# 1. Fetch latest changes: `git fetch origin`

# 2. Rebase onto {{DEFAULT_BRANCH}}:
#    - Run: `git rebase origin/{{DEFAULT_BRANCH}}`
#    - If conflicts occur, resolve them and continue rebase
#    - If rebase fails completely, abort with `git rebase --abort` and report the issue

# 3. Review commit history:
#    - Run: `git log origin/{{DEFAULT_BRANCH}}..HEAD --oneline`
#    - If there are many small fix commits, consider squashing them
#    - Keep meaningful commit boundaries (feature commits separate from fix commits)

# 4. Optional: Interactive rebase to clean up history:
#    - Only if there are 5+ commits that could be logically combined
#    - Run: `git rebase -i origin/{{DEFAULT_BRANCH}}`
#    - Squash related fix commits into their parent feature commits
#    - Reword commit messages if needed for clarity

# 5. Verify the branch is ready:
#    - Run tests using the project's test command (check CLAUDE.md or plan file for the correct command)
#    - Run linter if applicable

# Report what was done. This step is best-effort - if rebase fails, explain why and the branch remains as-is.

# OUTPUT FORMAT: No markdown formatting (no **bold**, `code`, # headers). Plain text and - lists are fine.
"#;

pub(crate) const AGENT_QUALITY_TXT: &str = r#"# Review code for bugs, security issues, and quality problems.

## Correctness Review

# 1. Logic errors - off-by-one errors, incorrect conditionals, wrong operators
# 2. Edge cases - empty inputs, nil/null values, boundary conditions, concurrent access
# 3. Error handling - all errors checked, appropriate error wrapping, no silent failures
# 4. Resource management - proper cleanup, no leaks, correct resource release
# 5. Concurrency issues - race conditions, deadlocks, thread/coroutine leaks
# 6. Data integrity - validation, sanitization, consistent state management

## Security Analysis

# 1. Input validation - all user inputs validated and sanitized
# 2. Authentication/authorization - proper checks in place
# 3. Injection vulnerabilities - SQL, command, path traversal
# 4. Secret exposure - no hardcoded credentials or keys
# 5. Information disclosure - error messages, logs, debug info

## Simplicity Assessment

# 1. Direct solutions first - if simple approach works, don't use complex pattern
# 2. No enterprise patterns for simple problems - avoid factories, builders for straightforward code
# 3. Question every abstraction - each interface/abstraction must solve real problem
# 4. No scope creep - changes solve only the stated problem
# 5. No premature optimization - unless addressing proven bottlenecks

## What to Report

# For each issue:
# - Location: exact file path and line number
# - Issue: clear description
# - Impact: how this affects the code
# - Fix: specific suggestion

# Focus on defects that would cause runtime failures, security vulnerabilities, or maintainability problems.
# Report problems only - no positive observations.
"#;

pub(crate) const AGENT_IMPLEMENTATION_TXT: &str = r#"# Review whether the implementation achieves the stated goal/requirement.

## Core Review Responsibilities

# 1. Requirement coverage - does implementation address all aspects of the stated requirement? Are there edge cases or scenarios not handled?

# 2. Correctness of approach - is the chosen approach actually solving the right problem? Could it fail to achieve the goal in certain conditions?

# 3. Wiring and integration - is everything connected properly? Are new components registered, routes added, handlers wired, configs updated?

# 4. Completeness - are there missing pieces that would prevent the feature from working? Missing imports, unimplemented interfaces, incomplete migrations?

# 5. Logic flow - does data flow correctly from input to output? Are transformations correct? Is state managed properly?

# 6. Edge cases - are boundary conditions handled? Empty inputs, null values, concurrent access, error paths?

## What to Report

# For each issue found:
# - Issue: clear description of what's wrong
# - Impact: how this prevents achieving the goal
# - Location: file and line reference
# - Fix: what needs to be added or changed

# Focus on correctness of approach, not code style.
# Report problems only - no positive observations.
"#;

pub(crate) const AGENT_TESTING_TXT: &str = r#"# Review test coverage and quality.

## Test Existence and Coverage

# 1. Missing tests - new code paths without corresponding tests
# 2. Untested error paths - error conditions not verified
# 3. Coverage gaps - functions or branches without test coverage
# 4. Integration test needs - system boundaries requiring integration tests

## Test Quality

# 1. Tests verify behavior, not implementation details
# 2. Each test is independent, can run in any order
# 3. Descriptive test names that explain what is being tested
# 4. Both success and error paths tested
# 5. Edge cases and boundary conditions covered

## Fake Test Detection

# Watch for tests that don't actually verify code:
# - Tests that always pass regardless of code changes
# - Tests checking hardcoded values instead of actual output
# - Tests verifying mock behavior instead of code using the mock
# - Ignored errors with _ or empty error checks
# - Conditional assertions that always pass
# - Commented out failing test cases

## Test Independence

# 1. No shared mutable state between tests
# 2. Proper setup and teardown
# 3. No order dependencies between tests
# 4. Resources properly cleaned up

## Edge Case Coverage

# 1. Empty inputs and collections
# 2. Null/nil values
# 3. Boundary values (zero, max, min)
# 4. Concurrent access scenarios
# 5. Timeout and cancellation handling

## What to Report

# For each finding:
# - Location: test file and function
# - Issue: what's wrong with the test
# - Impact: what bugs could slip through
# - Fix: how to improve the test

# Report problems only - no positive observations.
"#;

pub(crate) const AGENT_SIMPLIFICATION_TXT: &str = r#"# Detect over-engineered and overcomplicated code - code that works but is more complex than necessary.

## Excessive Abstraction Layers

# - Wrapper adds nothing - method just calls another method with same signature
# - Factory for single implementation - factory pattern when only one concrete type exists
# - Interface on producer side - interface defined where implemented, not where consumed
# - Layer cake anti-pattern - handler -> service -> repository when each just passes through
# - DTO/Mapper overkill - multiple types representing same data with conversion functions

## Premature Generalization

# - Generic solution for specific problem - event bus for one event type
# - Config objects for 2-3 options - options pattern when direct parameters suffice
# - Plugin architecture for fixed functionality - extension points nothing extends
# - Overloaded struct - one type handling all variations with many optional fields

## Unnecessary Indirection

# - Pass-through wrappers - methods that only delegate to dependencies
# - Excessive method chaining - builder pattern for simple constructions
# - Interface wrapping primitives - custom types for standard library types
# - Middleware stacking - multiple middlewares that could be one

## Future-Proofing Excess

# - Unused extension points - hooks, callbacks, plugins with no callers
# - Versioned internal APIs - v1/v2 when only one version used
# - Feature flags for permanent decisions - flags always on/off

## Unnecessary Fallbacks

# - Fallback that never triggers - default path conditions never met
# - Legacy mode kept just in case - old code path always disabled
# - Dual implementations - old + new logic when old has no callers
# - Silent fallbacks hiding problems - catching errors and falling back instead of failing fast

## Premature Optimization

# - Caching rarely-accessed data - cache for data read once at startup
# - Custom data structures - complex structures when arrays/maps work
# - Worker pools for occasional tasks - pooling for operations/hour
# - Connection pooling overkill - complex pooling for single connection

## What to Report

# For each finding:
# - Location: file and line reference
# - Pattern: which over-engineering pattern detected
# - Problem: why this adds unnecessary complexity
# - Simplification: what simpler code would look like
# - Effort: trivial/small/medium/large

# Report problems only - no positive observations.
"#;

pub(crate) const AGENT_DOCUMENTATION_TXT: &str = r#"# Review code changes and identify missing documentation updates.

## README.md (Human Documentation)

# Check if changes require README updates:

# Must document:
# - New features or capabilities
# - New CLI flags or command-line options
# - New API endpoints or interfaces
# - New configuration options
# - Changed behavior that affects users
# - New dependencies or system requirements
# - Breaking changes

# Skip:
# - Internal refactoring with no user-visible changes
# - Bug fixes that restore documented behavior
# - Test additions
# - Code style changes

## CLAUDE.md (AI Knowledge Base)

# Check if changes require CLAUDE.md updates:

# Must document:
# - New architectural patterns discovered/established
# - New conventions or coding standards
# - New build/test commands
# - New libraries or tools integrated
# - Project structure changes
# - Workflow changes
# - Non-obvious debugging techniques

# Skip:
# - Standard code additions following existing patterns
# - Simple bug fixes
# - Test additions using existing patterns

## Plan Files

# If changes relate to an existing plan:
# - Mark completed items as done
# - Update plan status if needed
# - Note which plan items this change addresses

## What to Report

# For each gap:
# - Missing: what needs to be documented
# - Section: where in the documentation it should go
# - Suggested content: draft text or outline

# Report problems only - no positive observations.
"#;
