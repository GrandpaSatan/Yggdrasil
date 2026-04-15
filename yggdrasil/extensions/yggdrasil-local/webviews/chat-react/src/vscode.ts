/**
 * Typed wrapper around the VS Code webview API.
 *
 * `acquireVsCodeApi()` may only be called ONCE per webview lifetime — subsequent
 * calls throw. The module-level cache here is the single source of truth.
 *
 * Message-type contract lives in `state/messages.ts` and is preserved bit-for-bit
 * from the classic webview so the extension-host handlers in `chatPanel.ts` do
 * not need to change their serialization.
 */

// VS Code injects this at load time.
declare function acquireVsCodeApi(): {
  postMessage<T = unknown>(message: T): void;
  getState<T = unknown>(): T | undefined;
  setState<T = unknown>(state: T): void;
};

let _api: ReturnType<typeof acquireVsCodeApi> | null = null;
function api() {
  if (!_api) _api = acquireVsCodeApi();
  return _api;
}

export function post<T extends { type: string }>(message: T): void {
  api().postMessage(message);
}

export function getPersistedState<T = unknown>(): T | undefined {
  return api().getState<T>();
}

export function setPersistedState<T = unknown>(state: T): void {
  api().setState(state);
}

export function onHostMessage(handler: (msg: unknown) => void): () => void {
  const listener = (event: MessageEvent) => handler(event.data);
  window.addEventListener("message", listener);
  return () => window.removeEventListener("message", listener);
}
