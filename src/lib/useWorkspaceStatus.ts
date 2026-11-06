import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { WorkspaceStatus } from "../contracts";
import { errorMessage } from "./format";

/** Subscribe before reading so reconnects cannot overwrite a newer event. */
export function useWorkspaceStatus(variantId: string) {
  const [status, setStatus] = useState<WorkspaceStatus | null>(null);
  const [error, setError] = useState("");
  const [settled, setSettled] = useState<{
    variantId: string;
    revision: number;
  } | null>(null);
  const current = useRef<WorkspaceStatus | null>(null);
  const acceptCurrent = useRef<((status: WorkspaceStatus) => void) | null>(
    null,
  );
  const target = useRef(variantId);
  target.current = variantId;
  useEffect(() => {
    let active = true;
    let connected = false;
    let reading = false;
    let unsubscribe: (() => void) | undefined;
    current.current = null;
    setStatus(null);
    setError("");
    setSettled(null);
    if (!variantId) return;
    const accept = (next: WorkspaceStatus) => {
      if (!active || next.variantId !== variantId) return;
      setError("");
      if (current.current && next.revision <= current.current.revision) return;
      current.current = next;
      setStatus(next);
      if (next.state !== "checking" && next.state !== "syncing")
        setSettled({ variantId, revision: next.revision });
    };
    acceptCurrent.current = accept;
    const read = async () => {
      if (!active || reading) return;
      reading = true;
      try {
        accept(await api.getWorkspaceStatus(variantId));
      } catch (reason) {
        if (active) setError(errorMessage(reason));
      } finally {
        reading = false;
      }
    };
    void (async () => {
      try {
        unsubscribe = await api.onWorkspaceStatus(accept);
        if (!active) {
          unsubscribe();
          return;
        }
        connected = true;
      } catch {
        // Read/poll still recovers status when event subscription is unavailable.
      }
      await read();
    })();
    const poll = setInterval(() => {
      if (
        !connected ||
        !current.current ||
        current.current.discardState === "pending" ||
        current.current.state === "checking" ||
        current.current.state === "syncing"
      )
        void read();
    }, 1500);
    return () => {
      active = false;
      clearInterval(poll);
      unsubscribe?.();
      if (acceptCurrent.current === accept) acceptCurrent.current = null;
    };
  }, [variantId]);
  const retry = useCallback(async () => {
    setError("");
    try {
      const next = await api.requestWorkspaceSync(variantId);
      if (target.current === variantId) acceptCurrent.current?.(next);
    } catch (reason) {
      if (target.current === variantId) setError(errorMessage(reason));
    }
  }, [variantId]);
  return {
    status: status?.variantId === variantId ? status : null,
    current,
    error,
    retry,
    settledRevision: settled?.variantId === variantId ? settled.revision : null,
  };
}
