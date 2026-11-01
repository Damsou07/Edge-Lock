import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Job, JobRequest } from "../contracts";

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
    let unsubscribe: (() => void) | undefined;
    void api
      .onJob(accept)
      .then((fn) => {
        if (alive.current) unsubscribe = fn;
        else fn();
      })
      .catch(() => {
        /* Polling still reports every job. */
      });
    return () => {
      alive.current = false;
      unsubscribe?.();
    };
  }, [accept]);
  const runJob = useCallback(
    async <T = unknown>(request: JobRequest): Promise<T> => {
      const id = await api.startJob(request);
      while (alive.current) {
        const job = await api.getJob(id);
        accept(job);
        if (job.status === "completed") return job.result as T;
        if (job.status === "failed")
          throw job.error ?? new Error("L’opération a échoué.");
        if (job.status === "cancelled") throw new Error("Opération annulée.");
        await new Promise((resolve) => setTimeout(resolve, 600));
      }
      throw new Error("La fenêtre a été fermée.");
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
