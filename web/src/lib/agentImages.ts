import { useSyncExternalStore } from 'react';

const event = 'tetonic-agent-image';
const key = (id: string) => `tetonic_agent_image:${id}`;
function read(id: string) {
  try {
    return localStorage.getItem(key(id));
  } catch {
    return null;
  }
}
function subscribe(notify: () => void) {
  window.addEventListener(event, notify);
  window.addEventListener('storage', notify);
  return () => {
    window.removeEventListener(event, notify);
    window.removeEventListener('storage', notify);
  };
}
export function useAgentImage(id: string) {
  return useSyncExternalStore(
    subscribe,
    () => read(id),
    () => null,
  );
}
export function saveAgentImage(id: string, image: string | null) {
  if (image) localStorage.setItem(key(id), image);
  else localStorage.removeItem(key(id));
  window.dispatchEvent(new Event(event));
}
export async function prepareAgentImage(file: File): Promise<string> {
  if (!['image/png', 'image/jpeg', 'image/webp'].includes(file.type)) {
    throw new Error('Choose a PNG, JPEG, or WebP image.');
  }
  if (file.size > 10 * 1024 * 1024) throw new Error('Choose an image smaller than 10 MB.');
  const bitmap = await createImageBitmap(file);
  try {
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 256;
    const context = canvas.getContext('2d');
    if (!context) throw new Error('Unable to prepare this image.');
    const side = Math.min(bitmap.width, bitmap.height);
    context.drawImage(
      bitmap,
      (bitmap.width - side) / 2,
      (bitmap.height - side) / 2,
      side,
      side,
      0,
      0,
      256,
      256,
    );
    return canvas.toDataURL('image/webp', 0.85);
  } finally {
    bitmap.close();
  }
}
