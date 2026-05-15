# RalphTerm Product Brief

RalphTerm is a control plane for interactive AI coding agents.

It treats the terminal as the stable interface. Claude Code, Codex, and future agents run as the vendors ship them: inside user-owned shells, with user-owned authentication, normal rate limits, and normal safety prompts. RalphTerm adds the missing production layer around those sessions.

## Positioning

**RalphTerm is the API layer for real terminal agents.**

Use it when you want autonomous engineering workflows without depending on private APIs or deprecated one-shot prompt modes.

## Core promise

If a human can run the agent in a terminal, RalphTerm can host, observe, and orchestrate that session through an API.

## Product pillars

### 1. Terminal-native

RalphTerm does not pretend the terminal is a legacy surface. The terminal is the product boundary. PTYs preserve the exact behavior the official CLIs expect.

### 2. Local-first

The daemon binds to localhost by default. Remote exposure requires explicit authentication, transport, and operator intent.

### 3. Vendor-respectful

The official CLI handles login, identity, safety prompts, model routing, and rate limits. RalphTerm never copies tokens or impersonates a provider API.

### 4. Workflow-ready

The API is built for long-running work: status, input, resize, cancel, transcript, events, and signals.

### 5. Auditable

Every run should leave a replayable transcript and structured event history. Operator decisions, approval prompts, and failures must be visible.

## First audience

- developers who use Claude Code or Codex daily
- teams evaluating autonomous issue-to-patch workflows
- maintainers who want agent runs with logs, controls, and review loops
- tool builders replacing fragile prompt-mode integrations

## First product surface

1. `ralphterm serve` local daemon
2. REST + WebSocket API
3. CLI helpers for starting and watching runs
4. Static dashboard for sessions, transcripts, and approval requests
5. Documentation and landing site

## Non-goals

- Provider API proxying
- Account sharing
- Credential storage
- Silent destructive approvals
- Hosted multi-tenant SaaS before the local product is solid
