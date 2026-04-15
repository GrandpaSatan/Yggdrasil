/**
 * Zustand chat store — single source of truth for the webview.
 *
 * Phase 2 expansion: `swarmSteps` moved onto the last assistant message so
 * folds travel with the thought they describe; `notificationCard` added for
 * the self-improvement toast; `attachment` handling covers
 * requestFilePicker → filePicked and host-side attachSelection/attachFile.
 */

import { create } from "zustand";
import { immer } from "zustand/middleware/immer";
import type { ChatMsg, FlowSummary, SwarmEvent, ThreadSummary } from "./messages";

export interface Attachment {
  label: string;
  content: string;
  path?: string;
}

export interface NotificationCardState {
  count: number;
  summaryTitles: string[];
}

export interface PendingSeed {
  text: string;
  run: boolean;
}

export interface ChatState {
  threads: ThreadSummary[];
  currentThreadId: string | null;
  messages: ChatMsg[];
  flows: FlowSummary[];
  streaming: boolean;
  attachments: Attachment[];
  notice: string | null;
  notificationCard: NotificationCardState | null;
  /**
   * One-shot input preload. Set by the host `seed` message; consumed by
   * ChatInput on mount/change which calls `consumePendingSeed()` to clear.
   */
  pendingSeed: PendingSeed | null;

  applyState(p: { threads: ThreadSummary[]; currentThreadId: string | null; flows: FlowSummary[] }): void;
  applyMessages(messages: ChatMsg[]): void;
  beginStream(): void;
  appendDelta(delta: string): void;
  handleSwarmEvent(ev: SwarmEvent): void;
  endStream(failed?: boolean): void;
  streamError(error: string): void;
  setNotice(text: string | null): void;
  addAttachment(a: Attachment): void;
  removeAttachment(label: string): void;
  clearAttachments(): void;
  showNotificationCard(card: NotificationCardState): void;
  clearNotificationCard(): void;
  setPendingSeed(seed: PendingSeed | null): void;
  consumePendingSeed(): PendingSeed | null;
}

export const useChatStore = create<ChatState>()(
  immer((set) => ({
    threads: [],
    currentThreadId: null,
    messages: [],
    flows: [],
    streaming: false,
    attachments: [],
    notice: null,
    notificationCard: null,
    pendingSeed: null,

    applyState(p) {
      set((s) => {
        s.threads = p.threads;
        s.currentThreadId = p.currentThreadId;
        s.flows = p.flows;
      });
    },

    applyMessages(messages) {
      set((s) => {
        s.messages = messages.map((m) => ({ ...m, swarmSteps: m.swarmSteps ?? [] }));
      });
    },

    beginStream() {
      set((s) => {
        s.streaming = true;
        s.messages.push({ role: "assistant", content: "", swarmSteps: [] });
      });
    },

    appendDelta(delta) {
      set((s) => {
        const last = s.messages[s.messages.length - 1];
        if (last && last.role === "assistant") {
          last.content += delta;
        }
      });
    },

    handleSwarmEvent(ev) {
      set((s) => {
        const last = s.messages[s.messages.length - 1];
        if (last && last.role === "assistant") {
          last.swarmSteps = [...(last.swarmSteps ?? []), ev];
        }
      });
    },

    endStream(failed) {
      set((s) => {
        s.streaming = false;
        if (failed) {
          const last = s.messages[s.messages.length - 1];
          if (last && last.role === "assistant" && last.content.length === 0) {
            s.messages.pop();
          }
        }
      });
    },

    streamError(error) {
      set((s) => {
        s.streaming = false;
        s.notice = `Stream error: ${error}`;
      });
    },

    setNotice(text) {
      set((s) => {
        s.notice = text;
      });
    },

    addAttachment(a) {
      set((s) => {
        if (!s.attachments.find((x) => x.label === a.label)) {
          s.attachments.push(a);
        }
      });
    },

    removeAttachment(label) {
      set((s) => {
        s.attachments = s.attachments.filter((a) => a.label !== label);
      });
    },

    clearAttachments() {
      set((s) => {
        s.attachments = [];
      });
    },

    showNotificationCard(card) {
      set((s) => {
        s.notificationCard = card;
      });
    },

    clearNotificationCard() {
      set((s) => {
        s.notificationCard = null;
      });
    },

    setPendingSeed(seed) {
      set((s) => {
        s.pendingSeed = seed;
      });
    },

    consumePendingSeed() {
      // Immer's draft lets us read+clear atomically in one `set` call.
      let taken: PendingSeed | null = null;
      set((s) => {
        taken = s.pendingSeed;
        s.pendingSeed = null;
      });
      return taken;
    },
  })),
);
