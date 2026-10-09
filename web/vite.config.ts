import { defineConfig, transformWithEsbuild } from 'vite';
import { readFileSync } from 'node:fs';
import react from '@vitejs/plugin-react';
import path from 'path';
import { checkArchitecture, formatFindings } from './architecture.mjs';

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [
    {
      name: 'saved-appearance-before-paint',
      async transformIndexHtml() {
        // Compile the same small, import-free module used by the picker. A normal
        // head script runs before the static boot screen, React or engine requests.
        const source = readFileSync(path.resolve(__dirname, 'src/lib/appearance.ts'), 'utf8');
        const { code } = await transformWithEsbuild(
          `${source}\nrestoreSavedAppearance();`,
          'appearance.ts',
          { format: 'iife', minify: true },
        );
        return [
          {
            tag: 'style',
            children: readFileSync(path.resolve(__dirname, 'src/palette-tokens.css'), 'utf8'),
            injectTo: 'head-prepend' as const,
          },
          {
            tag: 'script',
            attrs: { 'data-tetonic-appearance': '' },
            children: code,
            injectTo: 'head-prepend' as const,
          },
        ];
      },
    },
    react(),
    {
      name: 'workspace-production-boundary',
      apply: 'build',
      buildStart() {
        const findings = checkArchitecture();
        if (findings.length) this.error(formatFindings(findings));
      },
      generateBundle(_options, bundle) {
        const excluded =
          /\/dev\/|\/tests\/|\/store\/(mockData|graphMockData|workloadPresets|sampleLineageMissions|largeWorkspaces|workroomExamples)\.|\/components\/work\/(MissionDeck|DirectorExperimentView|Workroom|LocalWorkspace)\.|\/components\/workspace\/(Workspace|WorkspaceMap|WorkspacePanels|WorkFocus)\./;
        for (const output of Object.values(bundle)) {
          if (output.type !== 'chunk') continue;
          const forbidden = output.moduleIds.filter((id) =>
            excluded.test(id.replaceAll('\\', '/')),
          );
          if (forbidden.length)
            this.error(
              `The public workspace imports a preview-only module: ${forbidden.join(', ')}`,
            );
        }
      },
    },
  ],
  resolve: {
    preserveSymlinks: true,
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:3000',
        changeOrigin: true,
      },
      '/ws': {
        target: 'ws://127.0.0.1:3000',
        ws: true,
      },
    },
  },
});
