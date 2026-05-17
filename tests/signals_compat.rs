// Tests for the legacy review signal handling have been removed:
// the new ralphex-style runner does not surface --review-command into the
// review phase (it always derives the reviewer via locate_wrapper_script),
// and review pass/fail is decided by scanning the transcript for
// CRITICAL/MAJOR rather than by REVIEW_PASS / RALPHEX:REVIEW_DONE markers.
