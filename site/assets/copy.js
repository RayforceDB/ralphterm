// Adds a "Copy" button to every <pre> on the page.
//
// For shell-prompt snippets (lines starting with "$ " or "PS> "), the
// copied text strips those prefixes so a paste into a terminal Just
// Works. Multi-line snippets keep continuation lines unchanged.
(function () {
  'use strict';

  function stripPromptPrefix(text) {
    var lines = text.replace(/\r\n/g, '\n').split('\n');
    var cleaned = lines.map(function (line) {
      // Drop "$ ", "PS> ", and a single leading "> " continuation marker.
      return line
        .replace(/^\s*\$\s+/, '')
        .replace(/^\s*PS\s*>\s+/, '')
        .replace(/^\s*>\s+/, '');
    });
    return cleaned.join('\n').replace(/\n+$/, '');
  }

  function makeButton() {
    var btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'rt-copy-btn';
    btn.setAttribute('aria-label', 'Copy to clipboard');
    btn.textContent = 'copy';
    return btn;
  }

  function flash(btn, label, isError) {
    var original = btn.dataset.originalLabel || 'copy';
    btn.dataset.originalLabel = original;
    btn.textContent = label;
    btn.classList.toggle('is-error', !!isError);
    btn.classList.toggle('is-success', !isError);
    window.setTimeout(function () {
      btn.textContent = original;
      btn.classList.remove('is-success', 'is-error');
    }, 1400);
  }

  function copyText(text, btn) {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(
        function () { flash(btn, 'copied'); },
        function () { fallbackCopy(text, btn); }
      );
    } else {
      fallbackCopy(text, btn);
    }
  }

  function fallbackCopy(text, btn) {
    var ta = document.createElement('textarea');
    ta.value = text;
    ta.setAttribute('readonly', '');
    ta.style.position = 'absolute';
    ta.style.left = '-9999px';
    document.body.appendChild(ta);
    ta.select();
    var ok = false;
    try {
      ok = document.execCommand('copy');
    } catch (e) {
      ok = false;
    }
    document.body.removeChild(ta);
    flash(btn, ok ? 'copied' : 'failed', !ok);
  }

  function attach(pre) {
    if (pre.dataset.copyAttached) {
      return;
    }
    pre.dataset.copyAttached = '1';

    var wrapper = document.createElement('div');
    wrapper.className = 'rt-copy-wrap';
    pre.parentNode.insertBefore(wrapper, pre);
    wrapper.appendChild(pre);

    var btn = makeButton();
    wrapper.appendChild(btn);

    btn.addEventListener('click', function () {
      var source = pre.textContent || '';
      copyText(stripPromptPrefix(source), btn);
    });
  }

  function init() {
    var nodes = document.querySelectorAll('pre');
    for (var i = 0; i < nodes.length; i += 1) {
      attach(nodes[i]);
    }
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();
