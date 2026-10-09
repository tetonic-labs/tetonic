export function selectedWorkId() {
  return new URLSearchParams(window.location.hash.slice(1)).get('work');
}
export function selectWorkUrl(id: string | null) {
  const hash = id ? `#work=${encodeURIComponent(id)}` : '#main-content';
  window.history.pushState(null, '', window.location.pathname + window.location.search + hash);
}
