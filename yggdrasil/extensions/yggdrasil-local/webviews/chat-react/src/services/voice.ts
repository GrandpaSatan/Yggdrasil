/**
 * Voice service — mounts the existing (pre-React) voice-client.js +
 * voice-worklet.js into the React webview without porting them.
 *
 * The extension host's `getHtml()` still sets `data-voice-*` attributes on
 * <body> (see chatPanel.ts getHtml). We read them at mount time and inject
 * a nonced script tag for voice-client.js. The worklet URI is read by the
 * client itself via `document.body.dataset.voiceWorkletUri`.
 *
 * Opt-in: `yggdrasil.voice.enabled` (default false) on the extension side
 * controls whether `data-voice-enabled` is "true". If not, this hook is a
 * no-op and the `<VoiceButton>` component hides itself.
 */

import { useEffect, useState } from "react";

export interface VoiceState {
  enabled: boolean;
  mounted: boolean;
  client: unknown | null;
}

declare global {
  interface Window {
    VoiceClient?: {
      start(): void;
      stop(): void;
      toggle(): void;
      isActive(): boolean;
    };
  }
}

export function useVoiceService(): VoiceState {
  const [state, setState] = useState<VoiceState>({ enabled: false, mounted: false, client: null });

  useEffect(() => {
    const enabled = document.body.dataset.voiceEnabled === "true";
    if (!enabled) {
      setState({ enabled: false, mounted: false, client: null });
      return;
    }

    const src = document.body.dataset.voiceClientUri;
    if (!src) {
      // Host forgot to set the URI — bail gracefully.
      setState({ enabled: true, mounted: false, client: null });
      return;
    }

    // Find a nonce from any existing script tag in <head> so CSP lets us
    // append another script without a policy violation.
    const anyScript = document.querySelector<HTMLScriptElement>("script[nonce]");
    const nonce = anyScript?.nonce ?? anyScript?.getAttribute("nonce") ?? "";

    const tag = document.createElement("script");
    tag.src = src;
    if (nonce) tag.setAttribute("nonce", nonce);
    tag.async = true;
    tag.onload = () => {
      setState({ enabled: true, mounted: true, client: window.VoiceClient ?? null });
    };
    tag.onerror = () => {
      setState({ enabled: true, mounted: false, client: null });
    };
    document.body.appendChild(tag);

    return () => {
      if (tag.parentElement) tag.parentElement.removeChild(tag);
    };
  }, []);

  return state;
}
