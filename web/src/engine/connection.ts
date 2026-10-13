// Tab-scoped connection and draft identity. Never put a bearer in persistent storage.
const tokenKey = 'tetonic_local_session';
const scopeKey = 'tetonic_draft_session';
function rememberToken(token: string) {
  try {
    if (sessionStorage.getItem(tokenKey) !== token)
      sessionStorage.setItem(scopeKey, crypto.randomUUID());
    sessionStorage.setItem(tokenKey, token);
  } catch {
    /* An explicit connection still works for this mounted tab. */
  }
  return token;
}

export function acceptConnectionLink(value: string) {
  let link: URL;
  try {
    link = new URL(value.trim());
  } catch {
    throw new Error('Paste the full connection link from Tetonic, including the part after #.');
  }
  if (
    link.protocol !== 'http:' ||
    !['127.0.0.1', 'localhost'].includes(link.hostname) ||
    link.username ||
    link.password ||
    link.pathname !== '/'
  )
    throw new Error('Use the local connection link provided by Tetonic.');
  if (link.origin !== window.location.origin)
    throw new Error('This link is for a different local address. Open the full link in a new tab.');
  const fragment = new URLSearchParams(link.hash.slice(1));
  const token = fragment.get('connect') || '';
  if (fragment.getAll('connect').length !== 1 || !/^[a-f0-9]{64}$/.test(token))
    throw new Error('This link is missing its connection key. Copy a fresh link from Tetonic.');
  // Never navigate to pasted text, persist the full URL, or send it to an endpoint.
  return rememberToken(token);
}

export async function copyConnectionLink(token: string) {
  if (!/^[a-f0-9]{64}$/.test(token)) throw new Error('Reconnect before copying a connection link.');
  try {
    await navigator.clipboard.writeText(`${window.location.origin}/?engine=local#connect=${token}`);
  } catch {
    throw new Error(
      'Clipboard access is unavailable. Use the connection link from the engine launcher.',
    );
  }
}
export function connectionDraftScope() {
  try {
    let scope = sessionStorage.getItem(scopeKey);
    if (!scope) {
      scope = crypto.randomUUID();
      sessionStorage.setItem(scopeKey, scope);
    }
    return scope;
  } catch {
    return 'this-tab';
  }
}
export function takeConnectionToken() {
  const fragment = new URLSearchParams(window.location.hash.slice(1));
  const supplied = fragment.get('connect');
  if (supplied) {
    // Remove the credential from the visible URL before issuing requests.
    window.history.replaceState(null, '', window.location.pathname + window.location.search);
    if (!/^[a-f0-9]{64}$/.test(supplied)) return '';
    return rememberToken(supplied);
  }
  try {
    return sessionStorage.getItem(tokenKey) || '';
  } catch {
    return '';
  }
}
