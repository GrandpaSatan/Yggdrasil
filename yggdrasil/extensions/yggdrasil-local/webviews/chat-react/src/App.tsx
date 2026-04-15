/**
 * App — composes the full Fergus chat UI.
 *
 * Layout (top-to-bottom):
 *   ChatHeader    — thread selector + new/clear/delete
 *   MessageList   — scrolling messages with embedded swarm-step folds
 *   NoticeBanner  — inline error / notice strip
 *   NotificationCard — self-improvement suggestions (when present)
 *   ChatInput     — textarea + SlashMenu + InputToolbar + AttachmentChips
 *
 * Host-message handlers live here so every store reducer is paired with the
 * outbound `{ type: "ready" }` handshake that triggers the initial state
 * + messages push.
 */

import { useEffect } from "react";
import { VscThemeProvider } from "./theme/VscThemeProvider";
import { useChatStore } from "./state/chatStore";
import { useWebviewListener } from "./hooks/useWebviewListener";
import { post } from "./vscode";
import { useVoiceService } from "./services/voice";
import { ChatHeader } from "./components/Chat/ChatHeader";
import { MessageList } from "./components/Chat/MessageList";
import { ChatInput } from "./components/Editor/ChatInput";
import { NoticeBanner } from "./components/NoticeBanner";
import { NotificationCard } from "./components/NotificationCard";

export function App(): JSX.Element {
  const messages = useChatStore((s) => s.messages);
  const streaming = useChatStore((s) => s.streaming);
  const applyState = useChatStore((s) => s.applyState);
  const applyMessages = useChatStore((s) => s.applyMessages);
  const beginStream = useChatStore((s) => s.beginStream);
  const appendDelta = useChatStore((s) => s.appendDelta);
  const handleSwarmEvent = useChatStore((s) => s.handleSwarmEvent);
  const endStream = useChatStore((s) => s.endStream);
  const streamError = useChatStore((s) => s.streamError);
  const setNotice = useChatStore((s) => s.setNotice);
  const addAttachment = useChatStore((s) => s.addAttachment);
  const showNotificationCard = useChatStore((s) => s.showNotificationCard);

  // Voice service — mounts voice-client.js when yggdrasil.voice.enabled is true.
  // Currently drives nothing visible; a VoiceButton consumer can hook
  // `state.client` from this hook to trigger start/stop/toggle.
  useVoiceService();

  // Signal readiness once — the host responds with `state` + `messages`.
  useEffect(() => {
    post({ type: "ready" });
  }, []);

  useWebviewListener(
    (msg) => {
      switch (msg.type) {
        case "state":
          applyState(msg.state);
          break;
        case "messages":
          applyMessages(msg.messages);
          break;
        case "streamStart":
          beginStream();
          break;
        case "streamDelta":
          appendDelta(msg.delta);
          break;
        case "swarmEvent":
          handleSwarmEvent(msg.event);
          break;
        case "streamEnd":
          endStream(msg.failed);
          break;
        case "streamError":
          streamError(msg.error);
          break;
        case "notice":
          setNotice(msg.text);
          break;
        case "filePicked":
          addAttachment({ label: msg.label, path: msg.path, content: msg.content });
          break;
        case "attachment":
          addAttachment({ label: msg.label, path: msg.path, content: msg.content });
          break;
        case "showNotificationCard":
          showNotificationCard({ count: msg.count, summaryTitles: msg.summaryTitles });
          break;
        case "activeEditor":
          // Editor context is surfaced as an attachment-like chip labelled with the filename.
          // `autoInjectActiveEditor` config on the host controls whether this is sent.
          addAttachment({
            label: msg.filename,
            path: msg.uri,
            content: `// active editor: ${msg.filename} (${msg.language})`,
          });
          break;
        case "seed":
          // Seeded flow hint / prefilled text — Phase 3 wires the text into the
          // ChatInput via a store field; for now just surface a notice.
          if (msg.seed.flowHint) {
            setNotice(`Pinned flow: /${msg.seed.flowHint}`);
          }
          break;
      }
    },
    [
      applyState,
      applyMessages,
      beginStream,
      appendDelta,
      handleSwarmEvent,
      endStream,
      streamError,
      setNotice,
      addAttachment,
      showNotificationCard,
    ],
  );

  return (
    <VscThemeProvider>
      <div className="flex h-full flex-col bg-bg text-fg">
        <ChatHeader />
        <main className="min-h-0 flex-1 overflow-hidden">
          <MessageList messages={messages} streaming={streaming} />
        </main>
        <NoticeBanner />
        <NotificationCard />
        <ChatInput />
      </div>
    </VscThemeProvider>
  );
}
