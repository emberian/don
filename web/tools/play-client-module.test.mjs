import assert from 'node:assert/strict';
import { test } from 'node:test';

const EXPECTED_BOOT_BOUNDARY = 'play client reached its DOM boot boundary';

test('play client is a complete ESM module and reaches boot', async () => {
  const priorWindow = globalThis.window;
  const priorDocument = globalThis.document;
  const priorConsoleError = console.error;
  const requestedIds = [];
  const reportedErrors = [];
  globalThis.window = {};
  globalThis.document = {
    getElementById(id) {
      requestedIds.push(id);
      if (id === 'gl') throw new Error(EXPECTED_BOOT_BOUNDARY);
      return null;
    },
    createElement() {
      return { style: {}, textContent: '' };
    },
    body: { prepend() {} },
  };
  console.error = (error) => reportedErrors.push(error);
  try {
    await import(`../public/js/play/client.js?boot-boundary=${Date.now()}`);
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(requestedIds[0], 'gl');
    assert.match(globalThis.window.don.bootError, new RegExp(EXPECTED_BOOT_BOUNDARY));
    assert.equal(reportedErrors.length, 1);
    assert.match(String(reportedErrors[0]?.message), new RegExp(EXPECTED_BOOT_BOUNDARY));
  } finally {
    console.error = priorConsoleError;
    if (priorWindow === undefined) delete globalThis.window;
    else globalThis.window = priorWindow;
    if (priorDocument === undefined) delete globalThis.document;
    else globalThis.document = priorDocument;
  }
});
