import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { api } from "../api";
import type { WorkspaceStatus } from "../contracts";
import { useWorkspaceStatus } from "./useWorkspaceStatus";

vi.mock("../api", () => ({
  api: {
    onWorkspaceStatus: vi.fn(),
    getWorkspaceStatus: vi.fn(),
    requestWorkspaceSync: vi.fn(),
  },
}));
const ready: WorkspaceStatus = {
  variantId: "one",
  revision: 1,
  state: "ready",
  sourceChanged: false,
  canCreateSnapshot: false,
  artifactKey: "first",
  error: null,
};
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.onWorkspaceStatus).mockResolvedValue(() => {});
  vi.mocked(api.getWorkspaceStatus).mockResolvedValue(ready);
});

it("lit l’état même si la connexion aux événements est indisponible", async () => {
  vi.mocked(api.onWorkspaceStatus).mockRejectedValue(
    new Error("Listener unavailable"),
  );
  const { result } = renderHook(() => useWorkspaceStatus("one"));
  await waitFor(() => expect(result.current.status?.state).toBe("ready"));
  expect(result.current.error).toBe("");
});

it("récupère la fin d’une synchronisation dont l’événement a été perdu", async () => {
  vi.useFakeTimers();
  try {
    vi.mocked(api.getWorkspaceStatus)
      .mockResolvedValueOnce({ ...ready, state: "syncing", artifactKey: null })
      .mockResolvedValue({ ...ready, revision: 2 });
    const { result, unmount } = renderHook(() => useWorkspaceStatus("one"));
    await act(async () => {});
    expect(result.current.status?.state).toBe("syncing");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1500);
    });
    expect(result.current.status?.state).toBe("ready");
    unmount();
  } finally {
    vi.useRealTimers();
  }
});

it("récupère l’état d’une annulation différée sans événement", async () => {
  vi.useFakeTimers();
  try {
    vi.mocked(api.getWorkspaceStatus)
      .mockResolvedValueOnce({ ...ready, discardState: "pending" })
      .mockResolvedValue({ ...ready, revision: 2, discardState: null });
    const { result, unmount } = renderHook(() => useWorkspaceStatus("one"));
    await act(async () => {});
    expect(result.current.status?.discardState).toBe("pending");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1500);
    });
    expect(result.current.status?.discardState).toBeNull();
    unmount();
  } finally {
    vi.useRealTimers();
  }
});
