import assert from 'node:assert/strict';
import test from 'node:test';
import { analyzeModules, checkArchitecture } from './architecture.mjs';

function check(files, entries = ['src/main.tsx'], required = []) {
  return analyzeModules(new Map(Object.entries(files)), { entries, required });
}

test('current production graph satisfies the boundary contract', () => {
  assert.deepEqual(checkArchitecture(), []);
});
test('missing owner and unresolved imports fail instead of silently disabling checks', () => {
  assert.equal(check({}, [], ['src/engine/client.ts'])[0].rule, 'WEB-OWNER');
  assert.equal(check({ 'src/main.tsx': "import './missing';" })[0].rule, 'WEB-IMPORT');
});
test('transport belongs to the client, including alias references and qualified fetch', () => {
  for (const code of [
    'fetch(url)',
    'window.fetch(url)',
    "window['fetch'](url)",
    'const send = fetch; send(url)',
    'new WebSocket(url)',
  ])
    assert.ok(
      check({ 'src/main.tsx': code }).some((f) => f.rule === 'WEB-TRANSPORT'),
      code,
    );
  assert.deepEqual(check({ 'src/engine/client.ts': 'fetch(url)' }, ['src/engine/client.ts']), []);
  assert.deepEqual(check({ 'src/main.tsx': '// fetch(url)\nconst example = "fetch(url)";' }), []);
});
test('contracts cannot acquire executable behavior', () => {
  const file = 'src/engine/contracts.ts';
  assert.deepEqual(check({ [file]: 'export interface Work { id: string }' }, [file]), []);
  assert.ok(
    check({ [file]: 'export const work = { state: "completed" };' }, [file]).some(
      (f) => f.rule === 'WEB-CONTRACT',
    ),
  );
});
test('projections cannot reach transport through reexports or browser state through helpers', () => {
  const files = {
    'src/main.tsx': "import '@/engine/projections/workspace';",
    'src/engine/projections/workspace.ts': "import { data } from '../../lib/barrel';",
    'src/lib/barrel.ts': "export { data } from './helper';",
    'src/lib/helper.ts': 'export const data = sessionStorage.getItem("work");',
  };
  assert.ok(check(files).some((f) => f.rule === 'WEB-PROJECTION' && f.file.endsWith('helper.ts')));
  files['src/lib/helper.ts'] = "export { LocalEngine as data } from '../engine/client';";
  files['src/engine/client.ts'] = 'export class LocalEngine {}';
  assert.ok(check(files).some((f) => f.rule === 'WEB-PROJECTION'));
});
test('type-only presentation imports do not turn display helpers into runtime dependencies', () => {
  const files = {
    'src/main.tsx': "import './engine/projections/records';",
    'src/engine/projections/records.ts':
      "import { type Work } from '../../lib/viewTypes'; export const records = [];",
    'src/lib/viewTypes.ts':
      'export interface Work { id: string }\nexport const title = document.title;',
  };
  assert.deepEqual(check(files), []);
});
test('preview state is rejected through aliases and literal dynamic imports', () => {
  for (const code of ["import '@/store/mockData';", "import('./store/mockData');"])
    assert.ok(
      check({ 'src/main.tsx': code, 'src/store/mockData.ts': 'export const rows = [];' }).some(
        (f) => f.rule === 'WEB-PREVIEW',
      ),
    );
  assert.equal(check({ 'src/main.tsx': 'import(variable)' })[0].rule, 'WEB-IMPORT');
});
test('infrastructure does not import presentation or React behavior', () => {
  assert.ok(
    check(
      {
        'src/engine/client.ts': "import '../components/View';",
        'src/components/View.tsx': 'export const View = () => null;',
      },
      ['src/engine/client.ts'],
    ).some((f) => f.rule === 'WEB-DIRECTION'),
  );
  assert.ok(
    check({ 'src/engine/connection.ts': "import { useState } from 'react';" }, [
      'src/engine/connection.ts',
    ]).some((f) => f.rule === 'WEB-DIRECTION'),
  );
  assert.ok(
    check({ 'src/engine/connection.ts': "import React, { type ReactNode } from 'react';" }, [
      'src/engine/connection.ts',
    ]).some((f) => f.rule === 'WEB-DIRECTION'),
  );
});
