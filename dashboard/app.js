const runForm = document.querySelector('#run-form');
const runFormStatus = document.querySelector('#run-form-status');
const runsBody = document.querySelector('#runs-body');
const runsStatus = document.querySelector('#runs-status');
const sessionsBody = document.querySelector('#sessions-body');
const sessionsStatus = document.querySelector('#sessions-status');

function cell(text) {
  const td = document.createElement('td');
  td.textContent = text || '—';
  return td;
}

function linkCell(links) {
  const td = document.createElement('td');
  td.className = 'artifact-links';

  for (const link of links) {
    const anchor = document.createElement('a');
    anchor.href = link.href;
    anchor.textContent = link.label;
    anchor.setAttribute('aria-label', link.ariaLabel);
    td.append(anchor);
  }

  return td;
}

function latestEvent(run, eventTypes) {
  const events = Array.isArray(run.events) ? run.events : [];
  for (let index = events.length - 1; index >= 0; index -= 1) {
    if (eventTypes.includes(events[index].type)) {
      return events[index];
    }
  }
  return null;
}

function runGateLabel(run) {
  const gateEvent = latestEvent(run, [
    'task_committed',
    'task_succeeded',
    'task_marked_complete',
    'task_failed',
    'agent_retry_started',
    'review_failed',
    'review_passed',
    'review_started',
    'validation_passed',
    'validation_failed',
    'task_started',
  ]);

  if (!gateEvent) {
    if (run.status === 'succeeded') return 'complete';
    if (run.status === 'failed') return 'blocked';
    return run.phase || 'planning';
  }

  const task = gateEvent.task_number ? `T${gateEvent.task_number}` : 'run';
  switch (gateEvent.type) {
    case 'task_started':
      return `${task} implementing`;
    case 'validation_failed':
      return `${task} validation failed`;
    case 'validation_passed':
      return `${task} awaiting review`;
    case 'review_started':
      return `${task} reviewing`;
    case 'review_failed':
      return `${task} review retry/block`;
    case 'review_passed':
      return `${task} accepted by review`;
    case 'agent_retry_started':
      return `${task} retrying implementation`;
    case 'task_failed':
      return `${task} blocked`;
    case 'task_marked_complete':
      return `${task} marked complete`;
    case 'task_succeeded':
      return `${task} accepted`;
    case 'task_committed':
      return `${task} committed`;
    default:
      return gateEvent.type;
  }
}

function runArtifactCell(run) {
  return linkCell([
    {
      label: 'summary',
      href: `/v1/runs/${run.id}/summary`,
      ariaLabel: `Summary artifact for run ${run.id}`,
    },
    {
      label: 'json',
      href: `/v1/runs/${run.id}/summary.json`,
      ariaLabel: `Summary JSON artifact for run ${run.id}`,
    },
    {
      label: 'diff',
      href: `/v1/runs/${run.id}/diff`,
      ariaLabel: `Diff artifact for run ${run.id}`,
    },
    {
      label: 'progress',
      href: `/v1/runs/${run.id}/progress`,
      ariaLabel: `Progress artifact index for run ${run.id}`,
    },
    {
      label: 'events',
      href: `/v1/runs/${run.id}/events`,
      ariaLabel: `Event log for run ${run.id}`,
    },
  ]);
}

function renderEmptyRow(body, message, colSpan = 4) {
  const row = document.createElement('tr');
  const empty = document.createElement('td');
  empty.colSpan = colSpan;
  empty.className = 'empty-state';
  empty.textContent = message;
  row.append(empty);
  body.append(row);
}

function renderErrorRow(body, message, colSpan = 4) {
  const row = document.createElement('tr');
  const cellElement = document.createElement('td');
  cellElement.colSpan = colSpan;
  cellElement.textContent = message;
  row.append(cellElement);
  body.append(row);
}

function renderRunRows(runs) {
  runsBody.replaceChildren();

  if (!runs.length) {
    renderEmptyRow(runsBody, 'No runs yet.', 6);
    return;
  }

  for (const run of runs) {
    const row = document.createElement('tr');
    row.append(
      cell(run.id),
      cell(run.phase),
      cell(run.status),
      cell(runGateLabel(run)),
      cell(run.plan_path),
      runArtifactCell(run),
    );
    runsBody.append(row);
  }
}

function renderSessionRows(sessions) {
  sessionsBody.replaceChildren();

  if (!sessions.length) {
    renderEmptyRow(sessionsBody, 'No sessions yet.', 5);
    return;
  }

  for (const session of sessions) {
    const row = document.createElement('tr');
    row.append(
      cell(session.id),
      cell(session.agent),
      cell(session.status),
      cell(session.signal),
      cell(session.approval_pending ? 'Pending' : 'Clear'),
    );
    sessionsBody.append(row);
  }
}

async function fetchRunEvents(run) {
  try {
    const response = await fetch(`/v1/runs/${run.id}/events`);
    if (!response.ok) {
      return { ...run, events: [] };
    }
    const events = await response.json();
    return { ...run, events };
  } catch (_error) {
    return { ...run, events: [] };
  }
}

async function loadRuns() {
  try {
    runsStatus.textContent = 'Loading…';
    const response = await fetch('/v1/runs');
    if (!response.ok) {
      throw new Error(`GET /v1/runs failed with ${response.status}`);
    }
    const runs = await response.json();
    const runsWithEvents = await Promise.all(runs.map(fetchRunEvents));
    renderRunRows(runsWithEvents);
    runsStatus.textContent = `${runs.length} run${runs.length === 1 ? '' : 's'}`;
  } catch (error) {
    runsStatus.textContent = 'Error';
    runsBody.replaceChildren();
    renderErrorRow(runsBody, error.message, 6);
  }
}

async function loadSessions() {
  try {
    sessionsStatus.textContent = 'Loading…';
    const response = await fetch('/v1/sessions');
    if (!response.ok) {
      throw new Error(`GET /v1/sessions failed with ${response.status}`);
    }
    const sessions = await response.json();
    renderSessionRows(sessions);
    sessionsStatus.textContent = `${sessions.length} session${sessions.length === 1 ? '' : 's'}`;
  } catch (error) {
    sessionsStatus.textContent = 'Error';
    sessionsBody.replaceChildren();
    renderErrorRow(sessionsBody, error.message, 5);
  }
}

function optionalText(formData, name) {
  const value = String(formData.get(name) || '').trim();
  return value ? value : null;
}

function runRequestBody(form) {
  const formData = new FormData(form);
  const body = {
    require_review: formData.has('require_review'),
    dry_run: formData.has('dry_run'),
    no_commit: formData.has('no_commit'),
    max_review_retries: Number(formData.get('max_review_retries') || 1),
  };

  for (const name of ['plan_path', 'agent', 'agent_command', 'review_agent', 'review_command']) {
    const value = optionalText(formData, name);
    if (value) body[name] = value;
  }

  return body;
}

async function submitRunForm(event) {
  event.preventDefault();

  try {
    runFormStatus.textContent = 'Starting…';
    const response = await fetch('/v1/runs', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(runRequestBody(runForm)),
    });
    const responseBody = await response.json().catch(() => ({}));
    if (!response.ok) {
      throw new Error(responseBody.error || `POST /v1/runs failed with ${response.status}`);
    }

    runFormStatus.textContent = `Created ${responseBody.id || 'run'} (${responseBody.status || 'queued'})`;
    await loadRuns();
  } catch (error) {
    runFormStatus.textContent = error.message;
  }
}

if (runForm) {
  runForm.addEventListener('submit', submitRunForm);
}

loadRuns();
loadSessions();
