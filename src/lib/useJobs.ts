import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Job, JobRequest } from "../contracts";

export interface RunJobOptions {
  signal?: AbortSignal;
}

export function useJobs() {
  const [jobs, setJobs] = useState<Job[]>([]);
  const alive = useRef(true);
  const accept = useCallback((job: Job) => {
    if (!alive.current) return;
    setJobs((previous) =>
      [job, ...previous.filter((item) => item.id !== job.id)].slice(0, 15),
    );
  }, []);
  useEffect(() => {
    alive.current = true;
    let active = true;
    let unsubscribe: (() => void) | undefined;
    void api
      .onJob(accept)
      .then((fn) => {
        if (active) unsubscribe = fn;
        else fn();
      })
      .catch(() => {
        /* Polling still reports every job. */
      });
    return () => {
      active = false;
      alive.current = false;
      unsubscribe?.();
    };
  }, [accept]);
  const runJob = useCallback(
    async <T = unknown>(
      request: JobRequest,
      options?: RunJobOptions,
    ): Promise<T> => {
      const signal = options?.signal;
      signal?.throwIfAborted();
      const id = await api.startJob(request);
      let cancellation: Promise<void> | undefined;
      const cancel = () => {
        cancellation ??= api.cancelJob(id).catch(() => {
          // Still wait for terminal state before another operation uses the worker.
        });
      };
      signal?.addEventListener("abort", cancel, { once: true });
      if (signal?.aborted) cancel();
      try {
        while (alive.current || signal?.aborted) {
          const job = await api.getJob(id);
          accept(job);
          if (
            ["completed", "failed", "cancelled"].includes(job.status) &&
            signal?.aborted
          ) {
            await cancellation;
            if (
              job.status === "completed" &&
              request.kind === "thumbnail" &&
              typeof job.result === "object" &&
              job.result !== null &&
              "resourceToken" in job.result &&
              typeof job.result.resourceToken === "string"
            ) {
              await api
                .releasePreviewResources([job.result.resourceToken])
                .catch(() => {
                  /* The caller never receives this cancelled result to release it. */
                });
            }
            throw new DOMException("Opération annulée.", "AbortError");
          }
          if (job.status === "completed") return job.result as T;
          if (job.status === "failed")
            throw job.error ?? new Error("L’opération a échoué.");
          if (job.status === "cancelled") throw new Error("Opération annulée.");
          await new Promise((resolve) => setTimeout(resolve, 600));
        }
        cancel();
        throw new Error("La fenêtre a été fermée.");
      } finally {
        signal?.removeEventListener("abort", cancel);
      }
    },
    [accept],
  );
  const dismiss = useCallback(
    (id: string) =>
      setJobs((previous) => previous.filter((job) => job.id !== id)),
    [],
  );
  return { jobs, runJob, dismiss };
}

export type RunJob = ReturnType<typeof useJobs>["runJob"];
