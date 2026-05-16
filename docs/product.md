# RalphTerm product brief

RalphTerm is a replacement layer for brittle prompt-mode AI CLI automation.

The problem is simple. Many autonomous coding systems run an AI CLI with a one-shot prompt flag, parse the text, and hope the command exits cleanly. That works until the CLI gets interactive.

It can ask for login. It can ask for permission to edit files. It can print a rate-limit message. It can change formatting. It can need a follow-up answer. It can move more behavior into the terminal UI. When that happens, prompt-mode orchestration breaks.

RalphTerm treats the terminal as the stable boundary. It runs the official CLI in a real PTY, the same way a human would. Then it exposes session control through a local API.

## What RalphTerm owns

- session creation
- terminal input
- output streaming
- resize and cancel
- transcripts
- workflow signals
- approvals
- run history

## What RalphTerm does not own

- provider authentication
- account identity
- private provider APIs
- rate-limit bypasses
- hidden approval bypasses

The official CLI still owns those things. RalphTerm makes it possible to automate around the real interactive surface instead of depending on a prompt flag that may stop being reliable.

## Milestone 1

Milestone 1 is a complete replacement loop for autonomous coding runs:

1. Accept a task.
2. Create an isolated workspace.
3. Start a planning agent in a PTY.
4. Start an implementation agent in a PTY.
5. Detect questions, failures, approvals, and completion.
6. Run review.
7. Produce a patch, transcript, summary, and event log.

That is the core product. The website and docs should explain this first. Everything else supports it.
