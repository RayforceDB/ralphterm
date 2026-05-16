# RalphTerm research note: ralphex cross-review workflow

## Correction

Ralphex is not just a loop that launches agents against a plan.

The important product feature is a verification pipeline:

```text
plan tasks
  -> implementation agent
  -> first Claude review with multiple specialist reviewers
  -> external reviewer loop, usually Codex/custom tool
  -> Claude evaluates external findings and fixes real issues
  -> final Claude review for critical/major regressions
  -> optional finalize
```

The implementation agent is only the first phase. The product value is that work is not accepted until independent review phases stop finding actionable issues.

## What ralphex actually does

Source inspected locally under `/home/hetoku/work/ralphex`.

Key references:

- `pkg/processor/runner.go`
- `pkg/config/defaults/prompts/review_first.txt`
- `pkg/config/defaults/prompts/review_second.txt`
- `pkg/config/defaults/prompts/codex_review.txt`
- `pkg/config/defaults/prompts/codex.txt`
- `CLAUDE.md`

### Full mode phases

From `runner.go`:

```text
runFull:
1. task phase
2. first Claude review pass
3. Claude review loop for critical/major findings
4. external review loop: Codex or custom tool
5. Claude evaluates external findings and fixes confirmed issues
6. post-external Claude review loop
7. optional finalize
```

### Review pass 1

`review_first.txt` asks Claude to launch 5 parallel reviewers:

- quality
- implementation
- testing
- simplification
- documentation

Rules:

- reviewers inspect branch diff and source files
- report problems only
- Claude deduplicates findings
- Claude must verify every finding against actual code
- confirmed issues are fixed
- if fixes were made, no `REVIEW_DONE` signal is emitted
- another review iteration must verify the fixes

Important semantic:

```text
REVIEW_DONE means this iteration found zero issues.
It does not mean "I fixed the issues".
```

### External review loop

`codex_review.txt` sends the diff and progress context to an external reviewer.

Then `codex.txt` asks Claude to evaluate each finding:

- read the reported code
- trace callers and context
- check the plan
- decide valid vs invalid
- fix valid issues
- do not commit during fix rounds
- do not output `CODEX_REVIEW_DONE` after fixing
- only output `CODEX_REVIEW_DONE` when the external reviewer itself reports no actionable issues

This is the real cross-review loop.

### Final review

`review_second.txt` runs a narrower review with 2 agents:

- quality
- implementation

Focus:

- critical/major only
- verify every finding
- fix confirmed regressions
- repeat until no findings

## RalphTerm implication

Current RalphTerm M1 is missing the core ralphex value until it has review gates.

The target product loop should be:

```text
ralphterm run plan.md
  -> execute next task via real PTY agent
  -> run validation commands
  -> run independent review gate
  -> verifier says PASS only when no actionable issue remains
  -> mark [x]
  -> local checkpoint commit
  -> next task
```

For replacement parity, a task should not be considered complete just because the implementation agent printed `COMPLETED` and validation commands passed. It must also pass cross-review.

## RalphTerm M1 revised acceptance

Minimum useful cross-review slice:

1. Add `--review-command <cmd>` for tests and custom reviewers.
2. After implementation and validation, run reviewer in a fresh PTY with task, transcript, validation output, and `git diff` context.
3. Reviewer outputs one of:
   - `REVIEW_PASS`
   - `REVIEW_FAIL`
4. If `REVIEW_FAIL`, do not mark `[x]` and exit non-zero.
5. If `REVIEW_PASS`, mark `[x]` and checkpoint locally.
6. Add fake reviewer tests:
   - pass reviewer allows completion
   - fail reviewer blocks completion

Then grow toward ralphex parity:

- multiple review phases
- specialist reviewers
- external reviewer loop
- evaluate-and-fix loop
- stalemate/patience detection
- progress history passed between review iterations
- final review before release

## Push policy

Do not push every local checkpoint. Batch local commits and push only coherent release slices/new versions, because each push triggers CI cost.
