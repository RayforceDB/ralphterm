# RalphTerm Security Model

RalphTerm automates terminals. That is powerful and dangerous. The security model is intentionally conservative.

## Hard rules

- Do not store provider credentials.
- Do not copy CLI auth files.
- Do not emulate provider-private APIs.
- Do not bypass provider safety systems.
- Do not silently approve destructive actions.
- Bind to localhost by default.

## Authentication ownership

The official CLI owns authentication.

Users run normal setup commands such as:

```bash
claude auth login
codex login
```

RalphTerm only launches the installed CLI in the user's environment.

## Approval policy

Default: manual.

Milestone 1 policies:

- `manual`: never auto-send approval input
- `allow-readonly`: may approve clearly read-only commands after pattern match
- `allow-configured-patterns`: only approve exact operator-defined prompts/actions

Every automatic decision must be logged as an event without secrets.

## Local API exposure

The API can type into terminals, cancel processes, and read transcripts. Therefore:

- default bind is `127.0.0.1`
- public bind should require an auth token
- browser dashboard should use same-origin API or explicit token
- transcripts may contain sensitive project data

## Transcript handling

Raw transcripts are useful for audits but may contain secrets printed by tools. Planned behavior:

- keep raw transcript locally
- create a cleaned transcript for UI/search
- redact common secret patterns in summaries
- never upload transcripts by default

## Threats to design against

- malicious web page hitting localhost API
- remote unauthenticated API access
- accidental auto-approval of destructive commands
- prompt injection asking the agent to reveal secrets
- runaway sessions consuming CPU or API quota
- transcript leakage through logs or issue comments

## Mitigations for Milestone 1

- localhost-only default
- CORS tightened from permissive to explicit local origins
- optional bearer token
- session and idle timeouts
- per-session working directory allowlist
- transcript redaction pass
- approval event queue
- clear audit log
