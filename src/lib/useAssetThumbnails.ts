import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { ThumbnailResource, ThumbnailResult } from "../contracts";
import { errorMessage } from "./format";
import type { RunJob } from "./useJobs";

export interface ThumbnailState extends Partial<ThumbnailResource> {
  status: "waiting" | "loading" | "ready" | "error";
  error?: string;
}

async function release(tokens: string[]) {
  if (tokens.length) await api.releasePreviewResources(tokens).catch(() => {});
}

/** One cancellable producer for the gallery; cards own no model or WebGL context. */
export function useAssetThumbnails(
  assets: { id: string; updatedAt: string }[],
  library: string | undefined,
  enabled: boolean,
  runJob: RunJob,
) {
  const [states, setStates] = useState<Record<string, ThumbnailState>>({});
  const [wakeVersion, wake] = useState(0);
  const stateRef = useRef(states);
  stateRef.current = states;
  const mounted = useRef(false);
  const visible = useRef(new Set<string>());
  const attempted = useRef(new Set<string>());
  const retried = useRef(new Set<string>());
  const owned = useRef(new Set<string>());
  const epoch = useRef(0);
  const pauses = useRef(0);
  const previouslyEnabled = useRef(false);
  const current = useRef<{
    id: string;
    abort: AbortController;
    done: Promise<void>;
  } | null>(null);
  const pageKey = JSON.stringify([
    library,
    assets.map(({ id, updatedAt }) => [id, updatedAt]),
  ]);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    const generation = ++epoch.current;
    const resources = owned.current;
    attempted.current.clear();
    retried.current.clear();
    setStates({});
    if (library && assets.length) {
      void api
        .getAssetThumbnails(assets.map(({ id }) => id))
        .then(async (cached) => {
          const tokens = Object.values(cached).map(
            (value) => value.resourceToken,
          );
          if (epoch.current !== generation) {
            await release(tokens);
            return;
          }
          tokens.forEach((token) => owned.current.add(token));
          setStates((previous) => {
            const next = { ...previous };
            for (const [id, resource] of Object.entries(cached)) {
              if (!next[id]?.url)
                next[id] = {
                  ...next[id],
                  ...resource,
                  status: next[id]?.status ?? "ready",
                };
            }
            return next;
          });
        })
        .catch(() => {
          /* Cache misses and read failures do not block generation. */
        });
    }
    return () => {
      epoch.current = generation + 1;
      current.current?.abort.abort();
      const tokens = [...resources];
      resources.clear();
      void release(tokens);
    };
    // The key contains all source metadata used by this effect; array identity is irrelevant.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pageKey]);

  useEffect(() => {
    if (enabled && !previouslyEnabled.current) attempted.current.clear();
    previouslyEnabled.current = enabled;
    if (!enabled) current.current?.abort.abort();
    if (!enabled || !library || pauses.current || current.current) return;
    const generation = epoch.current;
    const candidate = assets.find(
      ({ id }) => visible.current.has(id) && !attempted.current.has(id),
    );
    if (!candidate) return;
    const id = candidate.id;
    attempted.current.add(id);
    const abort = new AbortController();
    setStates((previous) => ({
      ...previous,
      [id]: { ...previous[id], status: "loading" },
    }));
    const done = (async () => {
      let glbToken: string | undefined;
      let image: ThumbnailResource | undefined;
      try {
        const result = await runJob<ThumbnailResult>(
          { kind: "thumbnail", assetId: id },
          { signal: abort.signal },
        );
        if (result.state === "ready") image = result;
        else {
          glbToken = result.resourceToken;
          abort.signal.throwIfAborted();
          const { renderThumbnail } =
            await import("../components/thumbnail-renderer");
          const png = await renderThumbnail(result.preview.url, abort.signal);
          abort.signal.throwIfAborted();
          image = await api.storeThumbnail(glbToken, png);
        }
        if (abort.signal.aborted || epoch.current !== generation) {
          await release([image.resourceToken]);
          abort.signal.throwIfAborted();
          return;
        }
        const old = stateRef.current[id]?.resourceToken;
        owned.current.add(image.resourceToken);
        if (old && old !== image.resourceToken) {
          owned.current.delete(old);
          await release([old]);
        }
        if (epoch.current !== generation || abort.signal.aborted) {
          owned.current.delete(image.resourceToken);
          await release([image.resourceToken]);
          abort.signal.throwIfAborted();
          return;
        }
        const resource = image;
        setStates((previous) => ({
          ...previous,
          [id]: { ...resource, status: "ready" },
        }));
      } catch (reason) {
        if (epoch.current !== generation) return;
        if (abort.signal.aborted) {
          attempted.current.delete(id);
          setStates((previous) => ({
            ...previous,
            [id]: {
              ...previous[id],
              status: previous[id]?.url ? "ready" : "waiting",
            },
          }));
        } else {
          const message =
            (reason as { code?: string })?.code === "preview_ambiguous"
              ? "Choisissez la source dans la fiche de l’asset."
              : errorMessage(reason);
          setStates((previous) => ({
            ...previous,
            [id]: { ...previous[id], status: "error", error: message },
          }));
        }
      } finally {
        if (glbToken) await release([glbToken]);
        current.current = null;
        if (mounted.current) wake((value) => value + 1);
      }
    })();
    current.current = { id, abort, done };
    // Runs after every relevant render; refs keep cancellation separate from state updates.
  }, [enabled, library, pageKey, assets, runJob, wakeVersion]);

  const setVisible = useCallback((id: string, active: boolean) => {
    if (active) visible.current.add(id);
    else {
      visible.current.delete(id);
      if (current.current?.id === id) current.current.abort.abort();
    }
    wake((value) => value + 1);
  }, []);
  const pause = useCallback(async () => {
    ++pauses.current;
    const task = current.current;
    task?.abort.abort();
    await task?.done;
  }, []);
  const resume = useCallback(() => {
    pauses.current = Math.max(0, pauses.current - 1);
    wake((value) => value + 1);
  }, []);
  const imageFailed = useCallback(async (id: string) => {
    const resource = stateRef.current[id];
    if (!resource?.resourceToken) return;
    const generation = epoch.current;
    const token = resource.resourceToken;
    const retry = !retried.current.has(id);
    retried.current.add(id);
    if (retry) await api.invalidateThumbnail(token).catch(() => {});
    owned.current.delete(token);
    await release([token]);
    if (
      epoch.current !== generation ||
      stateRef.current[id]?.resourceToken !== token
    )
      return;
    setStates((previous) => ({
      ...previous,
      [id]: {
        status: retry ? "waiting" : "error",
        error: "Vignette indisponible. Ouvrez la fiche de l’asset.",
      },
    }));
    if (retry) attempted.current.delete(id);
    wake((value) => value + 1);
  }, []);
  return { states, setVisible, pause, resume, imageFailed };
}
