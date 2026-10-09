// Tab-scoped connection and draft identity. Never put a bearer in persistent storage.
const tokenKey = 'tetonic_local_session';
const scopeKey = 'tetonic_draft_session';
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
    try {
      if (sessionStorage.getItem(tokenKey) !== supplied)
        sessionStorage.setItem(scopeKey, crypto.randomUUID());
      sessionStorage.setItem(tokenKey, supplied);
    } catch {
      /* This tab can still connect. */
    }
    return supplied;
  }
  try {
    return sessionStorage.getItem(tokenKey) || '';
  } catch {
    return '';
  }
}
