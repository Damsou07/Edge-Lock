import { act, renderHook, waitFor } from "@testing-library/react";
import { StrictMode } from "react";
import { beforeEach, expect, it, vi } from "vitest";
import { api } from "../api";
import { useJobs } from "./useJobs";

vi.mock("../api", () => ({
  api: {
    startJob: vi.fn(),
    getJob: vi.fn(),
    cancelJob: vi.fn(),
    onJob: vi.fn(),
    releasePreviewResources: vi.fn(),
  },
}));
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.onJob).mockResolvedValue(() => {});
  vi.mocked(api.cancelJob).mockResolvedValue(undefined);
  vi.mocked(api.releasePreviewResources).mockResolvedValue(undefined);
});

it("désinscrit une écoute asynchrone arrivée après le premier cleanup de StrictMode", async () => {
  let first!: (unsubscribe: () => void) => void;
  let second!: (unsubscribe: () => void) => void;
  const unsubscribeFirst = vi.fn();
  const unsubscribeSecond = vi.fn();
  vi.mocked(api.onJob)
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          first = resolve;
        }),
    )
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          second = resolve;
        }),
    );
  const { unmount } = renderHook(useJobs, { wrapper: StrictMode });
  await act(async () => {
    first(unsubscribeFirst);
    second(unsubscribeSecond);
  });
  expect(unsubscribeFirst).toHaveBeenCalledTimes(1);
  expect(unsubscribeSecond).not.toHaveBeenCalled();
  unmount();
  expect(unsubscribeSecond).toHaveBeenCalledTimes(1);
});

it.each([
  { kind: "thumbnail", state: "ready" },
  { kind: "thumbnail", state: "render" },
  { kind: "preview", state: "render" },
] as const)(
  "libère le résultat $kind $state si l’annulation arrive après production",
  async ({ kind, state }) => {
    vi.mocked(api.startJob).mockResolvedValue("finished-job");
    let terminal!: (value: unknown) => void;
    vi.mocked(api.getJob).mockImplementation(
      () =>
        new Promise((resolve) => {
          terminal = resolve as typeof terminal;
        }),
    );
    const { result } = renderHook(useJobs);
    const abort = new AbortController();
    const task = result.current
      .runJob(
        kind === "thumbnail"
          ? { kind, assetId: "a" }
          : { kind, variantId: "v" },
        { signal: abort.signal },
      )
      .catch((reason: unknown) => reason);
    await waitFor(() =>
      expect(api.getJob).toHaveBeenCalledWith("finished-job"),
    );
    abort.abort();
    await act(async () =>
      terminal({
        id: "finished-job",
        kind,
        status: "completed",
        result: { state, resourceToken: "unclaimed", url: "cached.png" },
        error: null,
      }),
    );
    expect(await task).toMatchObject({ name: "AbortError" });
    expect(api.releasePreviewResources).toHaveBeenCalledWith(["unclaimed"]);
  },
);

it("annule même avant réception de l’identifiant et attend la fin du worker", async () => {
  let started!: (id: string) => void;
  let terminal!: (value: unknown) => void;
  vi.mocked(api.startJob).mockImplementation(
    () =>
      new Promise((resolve) => {
        started = resolve;
      }),
  );
  vi.mocked(api.getJob).mockImplementation(
    () =>
      new Promise((resolve) => {
        terminal = resolve as typeof terminal;
      }),
  );
  const { result } = renderHook(useJobs);
  const abort = new AbortController();
  let finished = false;
  const task = result.current
    .runJob({ kind: "thumbnail", assetId: "a" }, { signal: abort.signal })
    .catch((reason: unknown) => {
      finished = true;
      return reason;
    });
  abort.abort();
  await act(async () => started("late-job"));
  await waitFor(() => expect(api.cancelJob).toHaveBeenCalledWith("late-job"));
  expect(finished).toBe(false);
  await act(async () =>
    terminal({
      id: "late-job",
      kind: "thumbnail",
      status: "cancelled",
      result: null,
      error: null,
    }),
  );
  expect(await task).toMatchObject({ name: "AbortError" });
});
