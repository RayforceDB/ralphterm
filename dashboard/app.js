const runsBody = document.querySelector('#runs-body');
const runsStatus = document.querySelector('#runs-status');
const sessionsBody = document.querySelector('#sessions-body');
const sessionsStatus = document.querySelector('#sessions-status');

function cell(text) {
  const td = document.createElement('td');
  td.textContent = text || '—';
  return td;
}

function renderEmptyRow(body, message) {
  const row = document.createElement('tr');
  const empty = document.createElement('td');
  empty.colSpan = 4;
  empty.className = 'empty-state';
  empty.textContent = message;
  row.append(empty);
  body.append(row);
}

function renderErrorRow(body, message) {
  const row = document.createElement('tr');
  const cellElement = document.createElement('td');
  cellElement.colSpan = 4;
  cellElement.textContent = message;
  row.append(cellElement);
  body.append(row);
}

function renderRunRows(runs) {
  runsBody.replaceChildren();

  if (!runs.length) {
    renderEmptyRow(runsBody, 'No runs yet.');
    return;
  }

  for (const run of runs) {
    const row = document.createElement('tr');
    row.append(
      cell(run.id),
      cell(run.phase),
      cell(run.status),
      cell(run.plan_path),
    );
    runsBody.append(row);
  }
}

function renderSessionRows(sessions) {
  sessionsBody.replaceChildren();

  if (!sessions.length) {
    renderEmptyRow(sessionsBody, 'No sessions yet.');
    return;
  }

  for (const session of sessions) {
    const row = document.createElement('tr');
    row.append(
      cell(session.id),
      cell(session.agent),
      cell(session.status),
      cell(session.signal),
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
    renderErrorRow(runsBody, error.message);
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
    renderErrorRow(sessionsBody, error.message);
  }
}

loadRuns();
loadSessions();
