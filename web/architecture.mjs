// Source boundaries supplement the production bundle guard and behavioral tests.
// This is a TypeScript syntax/import check, not a security sandbox or proof that
// a projection is semantically truthful. It deliberately ignores string/comment
// examples and follows aliases, reexports and literal dynamic imports.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const required = [
  'src/main.tsx',
  'src/App.tsx',
  'src/context/LocalEngineContext.tsx',
  'src/components/team-work/TeamWorkspace.tsx',
  'src/engine/contracts.ts',
  'src/engine/client.ts',
  'src/engine/connection.ts',
  'src/engine/failure.ts',
  'src/engine/projections/agents.ts',
  'src/engine/projections/records.ts',
  'src/engine/projections/workspace.ts',
  'src/engine/projections/taskState.ts',
];
const preview = /^(dev|tests|src\/store)\//;
const ioNames = new Set(['fetch', 'WebSocket', 'XMLHttpRequest', 'EventSource', 'sendBeacon']);
const browserState = new Set(['window', 'document', 'localStorage', 'sessionStorage', 'indexedDB']);
const infrastructure = new Set([
  'src/engine/client.ts',
  'src/engine/connection.ts',
  'src/engine/failure.ts',
]);

function imports(source) {
  const found = [];
  function visit(node) {
    if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier) {
      const clause = node.importClause;
      const names = clause?.namedBindings;
      const typeOnly =
        node.isTypeOnly ||
        clause?.isTypeOnly ||
        (!clause?.name &&
          names &&
          ts.isNamedImports(names) &&
          names.elements.length > 0 &&
          names.elements.every((n) => n.isTypeOnly)) ||
        (ts.isExportDeclaration(node) &&
          node.exportClause &&
          ts.isNamedExports(node.exportClause) &&
          node.exportClause.elements.length > 0 &&
          node.exportClause.elements.every((n) => n.isTypeOnly));
      found.push({ spec: node.moduleSpecifier.text, typeOnly: !!typeOnly });
    }
    if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword) {
      const arg = node.arguments[0];
      found.push({ spec: arg && ts.isStringLiteralLike(arg) ? arg.text : null, typeOnly: false });
    }
    ts.forEachChild(node, visit);
  }
  visit(source);
  return found;
}

function resolveLocal(from, spec, modules) {
  const local = spec.startsWith('.')
    ? path.posix.join(path.posix.dirname(from), spec)
    : spec.startsWith('@/')
      ? `src/${spec.slice(2)}`
      : null;
  if (!local) return null;
  const target = path.posix.normalize(local);
  return (
    [target, `${target}.ts`, `${target}.tsx`, `${target}/index.ts`, `${target}/index.tsx`].find(
      (candidate) => modules.has(candidate),
    ) || target
  );
}

export function analyzeModules(modules, options = {}) {
  const findings = [];
  const report = (file, rule, message) => findings.push({ file, rule, message });
  const anchors = options.required ?? required;
  for (const file of anchors)
    if (!modules.has(file))
      report(
        file,
        'WEB-OWNER',
        'Required owner missing; update the architecture contract when moving it.',
      );
  const parsed = new Map();
  const edges = new Map();
  const pending = [...(options.entries ?? ['src/main.tsx']), ...anchors];
  while (pending.length) {
    const file = pending.pop();
    if (parsed.has(file) || !modules.has(file) || !/\.tsx?$/.test(file)) continue;
    const source = ts.createSourceFile(file, modules.get(file), ts.ScriptTarget.Latest, true);
    parsed.set(file, source);
    if (preview.test(file))
      report(file, 'WEB-PREVIEW', 'Production must not reach preview or fixture state.');
    const links = [];
    for (const { spec, typeOnly } of imports(source)) {
      if (spec === null) {
        report(
          file,
          'WEB-IMPORT',
          'Use a literal module path so production imports can be checked.',
        );
        continue;
      }
      const target = resolveLocal(file, spec, modules);
      links.push({ target, spec, typeOnly });
      if (target && !modules.has(target))
        report(file, 'WEB-IMPORT', `Unresolved local import: ${spec}`);
      if (target) pending.push(target);
      if (/^(axios|ky|superagent|openai|@anthropic-ai\/sdk|@google\/genai)(\/|$)/.test(spec))
        report(
          file,
          'WEB-TRANSPORT',
          'Provider and alternate HTTP clients do not belong in the workspace; use the engine client.',
        );
    }
    edges.set(file, links);
  }
  // Keep the runtime dependency closure of projections free of browser state,
  // components and transport, including indirection through a helper or barrel.
  const projections = new Set();
  const queue = [...parsed.keys()].filter((f) => f.startsWith('src/engine/projections/'));
  while (queue.length) {
    const file = queue.pop();
    if (projections.has(file)) continue;
    projections.add(file);
    for (const edge of edges.get(file) || [])
      if (edge.target && !edge.typeOnly) queue.push(edge.target);
  }
  for (const [file, source] of parsed) {
    const projection = projections.has(file);
    const contract = file === 'src/engine/contracts.ts';
    if (
      contract &&
      source.statements.some(
        (node) => !(ts.isInterfaceDeclaration(node) || ts.isTypeAliasDeclaration(node)),
      )
    )
      report(
        file,
        'WEB-CONTRACT',
        'Wire contracts contain only type declarations; put behavior in its owner.',
      );
    for (const edge of edges.get(file) || []) {
      if (
        infrastructure.has(file) &&
        edge.target &&
        !['src/engine/contracts.ts', 'src/engine/failure.ts'].includes(edge.target)
      )
        report(file, 'WEB-DIRECTION', `Transport/connection code cannot depend on ${edge.target}.`);
      if (
        (infrastructure.has(file) || projection) &&
        /^react(\/|$)/.test(edge.spec) &&
        !edge.typeOnly
      )
        report(file, 'WEB-DIRECTION', 'Engine boundaries must not depend on React behavior.');
      if (
        projection &&
        edge.target &&
        (infrastructure.has(edge.target) || /src\/(components|context)\//.test(edge.target))
      )
        report(file, 'WEB-PROJECTION', `Projection code must not depend on ${edge.target}.`);
    }
    function visit(node) {
      if (
        ts.isElementAccessExpression(node) &&
        ts.isStringLiteralLike(node.argumentExpression) &&
        ioNames.has(node.argumentExpression.text) &&
        file !== 'src/engine/client.ts'
      )
        report(
          file,
          'WEB-TRANSPORT',
          'Use LocalEngine instead of a computed browser transport reference.',
        );
      // Identifier references also catch `const request = fetch` and window.fetch.
      // Property/type declaration names are not network or storage operations.
      if (ts.isIdentifier(node)) {
        const parent = node.parent;
        const declarationName =
          (ts.isPropertySignature(parent) ||
            ts.isPropertyAssignment(parent) ||
            ts.isMethodDeclaration(parent) ||
            ts.isVariableDeclaration(parent) ||
            ts.isParameter(parent)) &&
          parent.name === node;
        if (!declarationName && ioNames.has(node.text) && file !== 'src/engine/client.ts')
          report(
            file,
            'WEB-TRANSPORT',
            `Use LocalEngine for ${node.text}; do not bypass its connection/error contract.`,
          );
        if (
          !declarationName &&
          (projection || file === 'src/engine/failure.ts') &&
          (browserState.has(node.text) || ['setTimeout', 'setInterval'].includes(node.text))
        )
          report(
            file,
            'WEB-PROJECTION',
            `Projection dependencies must not access ${node.text}; pass presentation inputs explicitly.`,
          );
      }
      ts.forEachChild(node, visit);
    }
    visit(source);
  }
  return findings;
}

export function checkArchitecture(root = path.dirname(fileURLToPath(import.meta.url))) {
  const modules = new Map();
  function walk(relative) {
    for (const entry of fs.readdirSync(path.join(root, relative), { withFileTypes: true })) {
      const name = path.posix.join(relative, entry.name);
      if (entry.isDirectory()) walk(name);
      else
        modules.set(
          name,
          /\.tsx?$/.test(name) ? fs.readFileSync(path.join(root, name), 'utf8') : '',
        );
    }
  }
  for (const dir of ['src', 'dev', 'tests']) if (fs.existsSync(path.join(root, dir))) walk(dir);
  return analyzeModules(modules);
}

export const formatFindings = (findings) =>
  findings.map((f) => `${f.rule} ${f.file}: ${f.message}`).join('\n');

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const findings = checkArchitecture();
  if (findings.length) {
    console.error(formatFindings(findings));
    process.exitCode = 1;
  } else console.log('Frontend architecture: OK');
}
