/**
 * useWebviewListener — typed subscription to extension-host messages.
 *
 * Adapted from continuedev/continue (Apache 2.0). See NOTICE.
 * Yggdrasil adaptation: narrows on our HostToWebview union instead of
 * Continue's FromCoreProtocol.
 */

import { useEffect } from "react";
import { onHostMessage } from "../vscode";
import type { HostToWebview } from "../state/messages";

export function useWebviewListener(
  handler: (msg: HostToWebview) => void,
  deps: ReadonlyArray<unknown> = [],
): void {
  useEffect(() => {
    const dispose = onHostMessage((raw) => {
      if (raw && typeof raw === "object" && "type" in raw) {
        handler(raw as HostToWebview);
      }
    });
    return dispose;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
}
