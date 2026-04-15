/**
 * Zustand chat store — single source of truth for the webview.
 *
 * Phase 1 scaffolding: structure + reducers land here; full message rendering
 * is wired up in Phase 2. Intentionally no `models` or `currentModel` state —
 * Fergus is one persona, Odin intent-routes the backend.
 */

import { create } from "zustand";
import { immer } from "zustand/middleware/immer";
import type { ChatMsg, FlowSummary, SwarmEvent, ThreadSummary } from "./messages";

export interface ChatState {
  threads: ThreadSummary[];
  currentThreadId: string | null;
  messages: ChatMsg[];
  flows: FlowSummary[];
  streaming: boolean;
  swarmSteps: SwarmEvent[];
  attachments: Array<{ label: string; content: string; path?: string }>;
  notice: string | null;

  applyState(p: { threads: ThreadSummary[]; currentThreadId: string | null; flows: FlowSummary[] }): void;
  applyMessages(messages: ChatMsg[]): void;
  beginStream(): void;
  appendDelta(delta: string): void;
  handleSwarmEvent(ev: SwarmEvent): void;
  endStream(failed?: boolean): void;
  streamError(error: string): void;
  setNotice(text: string | null): void;
  addAttachment(a: { label: string; content: string; path?: string }): void;
  removeAttachment(label: string): void;
  clearAttachments(): void;
}

export const useChatStore = create<ChatState>()(
  immer((set) => ({
    threads: [],
    currentThreadId: null,
    messages: [],
    flows: [],
    streaming: false,
    swarmSteps: [],
    attachments: [],
    notice: null,

    applyState(p) {
      set((s) => {
        s.threads = p.threads;
        s.currentThreadId = p.currentThreadId;
        s.flows = p.flows;
      });
    },

    applyMessages(messages) {
      set((s) => {
        s.messages = messages;
        s.swarmSteps = [];
      });
    },

    beginStream() {
      set((s) => {
        s.streaming = true;
        s.swarmSteps = [];
        s.messages.push({ role: "assistant", content: "" });
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
        s.swarmSteps.push(ev);
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
  })),
);
