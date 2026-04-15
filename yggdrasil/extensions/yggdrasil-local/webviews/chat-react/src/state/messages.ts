/**
 * Message-passing contract between the extension host and the webview.
 *
 * Preserved exactly from the Sprint 062 classic webview so `chatPanel.ts`
 * does not need to change its handlers. The `state` payload drops the
 * `models`/`defaultModel` fields (Fergus picks no model client-side —
 * Odin intent-routes). `themeChange` is removed entirely; theme now
 * derives from `data-vscode-theme-kind` on `<body>`.
 */

export interface FlowSummary {
  name: string;
  description?: string;
  trigger?: { Manual?: unknown; Cron?: string; Intent?: string };
}

export interface ThreadSummary {
  id: string;
  title: string;
  updated_at: number;
}

export interface ChatMsg {
  role: "user" | "assistant" | "system";
  content: string;
  ts?: number;
  /** Swarm steps observed during this assistant turn (populated by `swarmEvent` frames). */
  swarmSteps?: SwarmEvent[];
}

export interface SwarmEvent {
  phase: string;
  label?: string;
  detail?: string;
  model?: string;
  elapsed_ms?: number;
}

// ── Extension host → webview ─────────────────────────────────────────────
export type HostToWebview =
  | {
      type: "state";
      state: {
        threads: ThreadSummary[];
        currentThreadId: string | null;
        flows: FlowSummary[];
      };
    }
  | { type: "messages"; messages: ChatMsg[] }
  | { type: "streamStart"; model?: string; flow?: string }
  | { type: "streamDelta"; delta: string }
  | { type: "swarmEvent"; event: SwarmEvent }
  | { type: "streamEnd"; model?: string; flow?: string; failed?: boolean }
  | { type: "streamError"; error: string }
  | { type: "notice"; text: string }
  | { type: "seed"; seed: { flowHint?: string; text?: string; run?: boolean } }
  | { type: "attachment"; label: string; path?: string; content: string }
  | { type: "filePicked"; label: string; path: string; content: string }
  | { type: "showNotificationCard"; count: number; summaryTitles: string[] }
  | {
      type: "activeEditor";
      filename: string;
      language: string;
      uri: string;
    };

// ── Webview → extension host ─────────────────────────────────────────────
export type WebviewToHost =
  | { type: "ready" }
  | {
      type: "send";
      text: string;
      model?: string;
      flow?: string;
      attachments?: Array<{ label: string; content: string; path?: string }>;
    }
  | { type: "stop" }
  | { type: "newThread" }
  | { type: "switchThread"; id: string }
  | { type: "clearThread" }
  | { type: "deleteThread"; id?: string }
  | { type: "copy"; text: string }
  | { type: "requestThreads" }
  | { type: "loadThread"; id: string }
  | { type: "renameThread"; id: string; title: string }
  | { type: "searchThreads"; query: string }
  | { type: "exportThread"; id: string }
  | { type: "requestFilePicker" }
  | { type: "previewDiff"; path: string; proposed: string }
  | { type: "notifView" }
  | { type: "notifSnooze" }
  | { type: "notifDismiss" }
  | { type: "attachFile" }
  | { type: "attachSelection" };
