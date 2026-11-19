import assert from 'node:assert/strict';
import { readdir, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import test from 'node:test';

const here = new URL('.', import.meta.url);
const extensionsDir = new URL('../extensions/', here);

function element() {
  return {
    children: [],
    append(child) { this.children.push(child); },
    querySelector(selector) {
      if (selector === ':scope > .panel-error') return this.children.find((child) => child.className === 'panel-error') || null;
      return null;
    },
    setAttribute() {},
  };
}

globalThis.document = { createElement: element };

const { getPanels, guard, registerCommand, registerPanel } = await import('../js/core/registry.js');
const { runCommand } = await import('../js/core/commands.js');

test('an asynchronously rejected panel mount renders its error without an unhandled rejection', async () => {
  const id = 'test.async-rejection';
  registerPanel({
    id, title: 'Async rejection', region: 'toolbar',
    async mount() {
      await Promise.resolve();
      throw new Error('boom');
    },
  });
  const panel = getPanels().find((candidate) => candidate.id === id);
  const slot = element();
  let unhandled = null;
  const onUnhandled = (reason) => { unhandled = reason; };
  process.on('unhandledRejection', onUnhandled);
  try {
    guard(`Panel "${id}"`, slot, () => panel.mount(slot, {}));
    await new Promise(setImmediate);
  } finally {
    process.removeListener('unhandledRejection', onUnhandled);
  }
  assert.equal(unhandled, null, `unexpected unhandled rejection: ${unhandled}`);
  assert.equal(slot.querySelector(':scope > .panel-error')?.textContent, `Panel "${id}" failed: boom`);
});

test('an over-budget command run reports the owning command', () => {
  const id = 'test.slow-command';
  registerCommand({
    id, title: 'Slow command',
    run() {
      const deadline = performance.now() + 100;
      while (performance.now() < deadline) {}
    },
  });
  const messages = [];
  const original = console.error;
  console.error = (...args) => messages.push(args.join(' '));
  try {
    assert.equal(runCommand(id), true);
  } finally {
    console.error = original;
  }
  assert.ok(
    messages.some((message) => message.includes(`interaction budget exceeded by Command "${id}"`)),
    `expected an interaction-budget report, got: ${messages.join('\n')}`,
  );
});

function extensionViolations(source) {
  const imports = [
    ...source.matchAll(/\bfrom\s*["']([^"']+)["']/g),
    ...source.matchAll(/\bimport\s*\(\s*["']([^"']+)["']\s*\)/g),
  ].map((match) => match[1]);
  const forbiddenImport = imports.find((specifier) => specifier.startsWith('../js/core/') && !['../js/core/dom.js', '../js/core/registry.js'].includes(specifier));
  const forbiddenCapability = source.match(/\bfetch\b|\bXMLHttpRequest\b|\bEventSource\b|\bWebSocket\b|\/api\//);
  return [
    ...(forbiddenImport ? [`forbidden core import ${forbiddenImport}`] : []),
    ...(forbiddenCapability ? [`forbidden network capability ${forbiddenCapability[0]}`] : []),
  ];
}

async function scanExtensions(directory = extensionsDir) {
  const files = (await readdir(directory)).filter((file) => file.endsWith('.js'));
  const violations = [];
  for (const file of files) {
    const source = await readFile(new URL(file, directory), 'utf8');
    for (const violation of extensionViolations(source)) violations.push(`${file}: ${violation}`);
  }
  return violations;
}

test('built-in extensions use only the registry contract and have no direct network write path', async () => {
  assert.deepEqual(await scanExtensions(), []);

  const fixture = join(extensionsDir.pathname, '.extension-boundary-test-fixture.js');
  await writeFile(fixture, "import { commit } from '../js/core/api.js';\nfetch('/api/commit');\nvoid commit;\n");
  try {
    assert.deepEqual(await scanExtensions(), [
      '.extension-boundary-test-fixture.js: forbidden core import ../js/core/api.js',
      '.extension-boundary-test-fixture.js: forbidden network capability fetch',
    ]);
  } finally {
    await rm(fixture, { force: true });
  }
});
