import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import test, { before } from 'node:test';
import { JSDOM } from 'jsdom';
import { createServer } from 'vite';

let html;
before(async () => {
  const root = fileURLToPath(new URL('.', import.meta.url));
  const server = await createServer({
    root,
    server: { middlewareMode: true, watch: null, hmr: false, preTransformRequests: false },
    appType: 'custom',
  });
  try {
    html = await server.transformIndexHtml(
      '/',
      readFileSync(new URL('index.html', import.meta.url), 'utf8'),
    );
  } finally {
    await server.close();
  }
});

// Run the actual generated head script, without loading React or the engine.
function openPage(values = {}, storageUnavailable = false) {
  return new JSDOM(
    html.replace(
      '</head>',
      `<script>
    window.appearanceBeforeBody = {
      palette: document.documentElement.dataset.palette,
      dark: document.documentElement.classList.contains('dark'),
      hasWorkspace: !!document.getElementById('root'),
      ground: getComputedStyle(document.documentElement).getPropertyValue('--bg-ground').trim(),
      ink: getComputedStyle(document.documentElement).getPropertyValue('--text-primary').trim()
    };
  </script></head>`,
    ),
    {
      url: 'http://localhost/',
      runScripts: 'dangerously',
      beforeParse(window) {
        if (storageUnavailable) {
          Object.defineProperty(window, 'localStorage', {
            get() {
              throw new Error('Storage disabled');
            },
          });
        } else {
          for (const [key, value] of Object.entries(values))
            window.localStorage.setItem(key, value);
        }
      },
    },
  );
}

test('saved presets apply before any loading or workspace UI is parsed', () => {
  for (const [palette, ground] of Object.entries({
    shrigley: '#f7f4e4',
    warhol: '#eaf2ec',
    electric: '#f5eff7',
    coral: '#f7efe2',
  })) {
    const page = openPage({ tetonic_palette_exp: palette });
    try {
      const state = page.window.appearanceBeforeBody;
      assert.equal(state.palette, palette);
      assert.equal(state.ground, ground);
      assert.equal(state.hasWorkspace, false);
    } finally {
      page.window.close();
    }
  }
});

test('custom colors survive a reload and take precedence over a saved dark-mode preference', () => {
  const values = {
    tetonic_palette_exp: 'custom',
    tetonic_palette_custom_colors: JSON.stringify({
      accent: '#0538ff',
      highlight: '#ffd100',
      ground: '#faf8f3',
    }),
    tetonic_appearance_dark: 'true',
  };
  for (let reload = 0; reload < 2; reload++) {
    const page = openPage(values);
    try {
      assert.equal(page.window.appearanceBeforeBody.palette, 'custom');
      assert.equal(page.window.appearanceBeforeBody.dark, true);
      assert.equal(page.window.appearanceBeforeBody.ground, '#faf8f3');
      assert.equal(page.window.appearanceBeforeBody.ink, '#121212');
    } finally {
      page.window.close();
    }
  }
});

test('saved dark mode and dark custom surfaces have readable startup text', () => {
  const dark = openPage({ tetonic_appearance_dark: 'true', tetonic_palette_exp: 'warhol' });
  const custom = openPage({
    tetonic_palette_exp: 'custom',
    tetonic_palette_custom_colors: JSON.stringify({
      accent: '#00b4d8',
      highlight: '#ffd100',
      ground: '#121212',
    }),
  });
  try {
    assert.equal(dark.window.appearanceBeforeBody.ground, '#1d211c');
    assert.equal(dark.window.appearanceBeforeBody.ink, '#efefe3');
    assert.equal(custom.window.appearanceBeforeBody.ground, '#121212');
    assert.equal(custom.window.appearanceBeforeBody.ink, '#faf8f3');
  } finally {
    dark.window.close();
    custom.window.close();
  }
});

test('malformed saved colors cannot inject CSS or stop startup', () => {
  for (const saved of [
    '{',
    JSON.stringify({
      accent: 'red; } body { display:none',
      highlight: '#ffd100',
      ground: '#121212',
    }),
  ]) {
    const page = openPage({ tetonic_palette_exp: 'custom', tetonic_palette_custom_colors: saved });
    try {
      assert.equal(page.window.appearanceBeforeBody.ground, '#faf8f3');
      assert.ok(
        !page.window.document
          .getElementById('tetonic-custom-palette-style')
          .textContent.includes('display:none'),
      );
    } finally {
      page.window.close();
    }
  }
});

test('unavailable storage or an unknown palette still opens with the default appearance', () => {
  for (const page of [openPage({}, true), openPage({ tetonic_palette_exp: 'retired-palette' })]) {
    try {
      assert.equal(page.window.appearanceBeforeBody.palette, undefined);
      assert.equal(page.window.appearanceBeforeBody.dark, false);
      assert.ok(page.window.document.querySelector('.preload-boot'));
    } finally {
      page.window.close();
    }
  }
});
