const runsBody = document.querySelector('#runs-body');
const runsStatus = document.querySelector('#runs-status');

function cell(text) {
  const td = document.createElement('td');
  td.textContent = text || '—';
  return td;
}

function renderRunRows(runs) {
  runsBody.replaceChildren();

  if (!runs.length) {
    const row = document.createElement('tr');
    const empty = document.createElement('td');
    empty.colSpan = 4;
    empty.className = 'empty-state';
    empty.textContent = 'No runs yet.';
    row.append(empty);
    runsBody.append(row);
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
    const row = document.createElement('tr');
    const message = document.createElement('td');
    message.colSpan = 4;
    message.textContent = error.message;
    row.append(message);
    runsBody.append(row);
  }
}

loadRuns();
