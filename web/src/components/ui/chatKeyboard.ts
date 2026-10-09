import type { KeyboardEvent } from 'react';

export function submitChatOnEnter(event: KeyboardEvent<HTMLTextAreaElement>) {
  if (
    event.defaultPrevented ||
    event.key !== 'Enter' ||
    event.shiftKey ||
    event.nativeEvent.isComposing ||
    event.nativeEvent.keyCode === 229
  )
    return;

  event.preventDefault();
  if (!event.repeat) event.currentTarget.form?.requestSubmit();
}
