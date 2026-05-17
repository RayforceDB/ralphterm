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
    renderEmptyRow(runsBody, 'No runs yet.', 5);
    return;
  }

  for (const run of runs) {
    const row = document.createElement('tr');
    row.append(
      cell(run.id),
      cell(run.phase),
      cell(run.status),
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

async function loadRuns() {
  try {
    runsStatus.textContent = 'Loading…';
    const response = await fetch('/v1/runs');
    if (!response.ok) {
      throw new Error(`GET /v1/runs failed with ${response.status}`);
    }
    const runs = await response.json();
    renderRunRows(runs);
    runsStatus.textContent = `${runs.length} run${runs.length === 1 ? '' : 's'}`;
  } catch (error) {
    runsStatus.textContent = 'Error';
    runsBody.replaceChildren();
    renderErrorRow(runsBody, error.message, 5);
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

loadRuns();
loadSessions();
