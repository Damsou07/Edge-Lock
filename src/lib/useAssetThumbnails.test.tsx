import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../api";
import type { JobRequest, ThumbnailResult } from "../contracts";
import type { RunJob, RunJobOptions } from "./useJobs";
import { useAssetThumbnails } from "./useAssetThumbnails";
import { renderThumbnail } from "../components/thumbnail-renderer";

vi.mock("../api", () => ({
  api: {
    getAssetThumbnails: vi.fn(),
    storeThumbnail: vi.fn(),
    releasePreviewResources: vi.fn(),
    invalidateThumbnail: vi.fn(),
  },
}));
vi.mock("../components/thumbnail-renderer", () => ({
  renderThumbnail: vi.fn(),
}));

const run =
  vi.fn<(request: JobRequest, options?: RunJobOptions) => Promise<unknown>>();
const runJob: RunJob = async <T,>(
  request: JobRequest,
  options?: RunJobOptions,
) => (await run(request, options)) as T;
const assets = [
  { id: "a", updatedAt: "one" },
  { id: "b", updatedAt: "one" },
];
const ready = (id: string): ThumbnailResult => ({
  state: "ready",
  url: `http://edge-preview.localhost/${id}`,
  resourceToken: id,
});

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getAssetThumbnails).mockResolvedValue({});
  vi.mocked(api.releasePreviewResources).mockResolvedValue(undefined);
  vi.mocked(api.invalidateThumbnail).mockResolvedValue(undefined);
  run.mockImplementation(
    async (request: JobRequest) =>
      ready(
        "assetId" in request ? (request.assetId ?? "unknown") : "unknown",
      ) as never,
  );
});

describe("file des vignettes", () => {
  it("affiche le cache groupé sans attendre les jobs et ignore les cartes invisibles", async () => {
    vi.mocked(api.getAssetThumbnails).mockResolvedValue({
      a: { url: "cached.png", resourceToken: "cached" },
    });
    const { result, unmount } = renderHook(() =>
      useAssetThumbnails(assets, "library", true, runJob),
    );
    await waitFor(() =>
      expect(result.current.states.a?.url).toBe("cached.png"),
    );
    expect(api.getAssetThumbnails).toHaveBeenCalledWith(["a", "b"]);
    expect(run).not.toHaveBeenCalled();
    unmount();
    await waitFor(() =>
      expect(api.releasePreviewResources).toHaveBeenCalledWith(["cached"]),
    );
  });

  it("prépare uniquement les cartes visibles, une à la fois", async () => {
    let finish!: (value: ThumbnailResult) => void;
    run.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve as typeof finish;
        }),
    );
    const { result } = renderHook(() =>
      useAssetThumbnails(assets, "library", true, runJob),
    );
    act(() => {
      result.current.setVisible("a", true);
      result.current.setVisible("b", true);
    });
    await waitFor(() => expect(run).toHaveBeenCalledTimes(1));
    expect(run).toHaveBeenCalledWith(
      { kind: "thumbnail", assetId: "a" },
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
    await act(async () => finish(ready("a")));
    await waitFor(() => expect(run).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(result.current.states.b?.url).toContain("/b"));
  });

  it("annule et attend le nettoyage avant de rendre la priorité à la fiche", async () => {
    let cancelled = false;
    run.mockImplementationOnce(
      (_request, options) =>
        new Promise((_resolve, reject) => {
          options?.signal?.addEventListener("abort", () => {
            cancelled = true;
            reject(new DOMException("Annulé", "AbortError"));
          });
        }),
    );
    const { result } = renderHook(() =>
      useAssetThumbnails(assets, "library", true, runJob),
    );
    act(() => result.current.setVisible("a", true));
    await waitFor(() => expect(run).toHaveBeenCalledTimes(1));
    await act(async () => {
      await result.current.pause();
    });
    expect(cancelled).toBe(true);
    expect(run).toHaveBeenCalledTimes(1);
    act(() => result.current.resume());
    await waitFor(() => expect(result.current.states.a?.status).toBe("ready"));
  });

  it("libère un résultat obsolète après changement de page", async () => {
    let finish!: (value: ThumbnailResult) => void;
    run.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve as typeof finish;
        }),
    );
    const { result, rerender } = renderHook(
      ({ items }) => useAssetThumbnails(items, "library", true, runJob),
      { initialProps: { items: assets } },
    );
    act(() => result.current.setVisible("a", true));
    act(() => result.current.setVisible("b", true));
    await waitFor(() => expect(run).toHaveBeenCalledTimes(1));
    rerender({ items: [assets[1]!] });
    await act(async () => finish(ready("obsolete")));
    await waitFor(() =>
      expect(api.releasePreviewResources).toHaveBeenCalledWith(["obsolete"]),
    );
    expect(result.current.states.a).toBeUndefined();
    await waitFor(() => expect(result.current.states.b?.status).toBe("ready"));
  });

  it("capture les textures une fois et libère le GLB après publication du PNG", async () => {
    run.mockResolvedValueOnce({
      state: "render",
      resourceToken: "glb",
      preview: { url: "model.glb" },
    } as never);
    vi.mocked(renderThumbnail).mockResolvedValue([137, 80, 78, 71]);
    vi.mocked(api.storeThumbnail).mockResolvedValue({
      url: "image.png",
      resourceToken: "png",
    });
    const { result } = renderHook(() =>
      useAssetThumbnails(assets, "library", true, runJob),
    );
    act(() => result.current.setVisible("a", true));
    await waitFor(() => expect(result.current.states.a?.url).toBe("image.png"));
    expect(renderThumbnail).toHaveBeenCalledWith(
      "model.glb",
      expect.any(AbortSignal),
    );
    expect(api.storeThumbnail).toHaveBeenCalledWith("glb", [137, 80, 78, 71]);
    expect(api.releasePreviewResources).toHaveBeenCalledWith(["glb"]);
  });

  it("ne republie pas une ancienne carte si la libération de sa vignette attend", async () => {
    vi.mocked(api.getAssetThumbnails).mockResolvedValueOnce({
      a: { url: "old.png", resourceToken: "old" },
    });
    let releaseOld!: () => void;
    vi.mocked(api.releasePreviewResources).mockImplementation((tokens) =>
      tokens.includes("old")
        ? new Promise<void>((resolve) => {
            releaseOld = resolve;
          })
        : Promise.resolve(),
    );
    const { result, rerender } = renderHook(
      ({ items }) => useAssetThumbnails(items, "library", true, runJob),
      { initialProps: { items: assets } },
    );
    await waitFor(() => expect(result.current.states.a?.url).toBe("old.png"));
    act(() => result.current.setVisible("a", true));
    await waitFor(() => expect(releaseOld).toBeDefined());
    rerender({ items: [assets[1]!] });
    await act(async () => releaseOld());
    expect(result.current.states.a).toBeUndefined();
    expect(api.releasePreviewResources).toHaveBeenCalledWith(["a"]);
  });

  it("régénère une image indécodable une seule fois", async () => {
    const { result } = renderHook(() =>
      useAssetThumbnails(assets, "library", true, runJob),
    );
    act(() => result.current.setVisible("a", true));
    await waitFor(() => expect(result.current.states.a?.status).toBe("ready"));
    await act(async () => result.current.imageFailed("a"));
    await waitFor(() => expect(run).toHaveBeenCalledTimes(2));
    await act(async () => result.current.imageFailed("a"));
    expect(run).toHaveBeenCalledTimes(2);
    expect(result.current.states.a?.status).toBe("error");
    expect(api.invalidateThumbnail).toHaveBeenCalledTimes(1);
  });

  it("conserve la nouvelle vignette si l’erreur de l’ancienne se termine plus tard", async () => {
    vi.mocked(api.getAssetThumbnails).mockResolvedValueOnce({
      a: { url: "old.png", resourceToken: "old" },
    });
    let finishGeneration!: (value: ThumbnailResult) => void;
    run.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishGeneration = resolve as typeof finishGeneration;
        }),
    );
    let finishInvalidation!: () => void;
    vi.mocked(api.invalidateThumbnail).mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finishInvalidation = resolve;
        }),
    );
    const { result } = renderHook(() =>
      useAssetThumbnails(assets, "library", true, runJob),
    );
    await waitFor(() => expect(result.current.states.a?.url).toBe("old.png"));
    act(() => result.current.setVisible("a", true));
    await waitFor(() => expect(run).toHaveBeenCalledTimes(1));
    let failure!: Promise<void>;
    act(() => {
      failure = result.current.imageFailed("a");
    });
    expect(api.invalidateThumbnail).toHaveBeenCalledWith("old");
    await act(async () => finishGeneration(ready("fresh")));
    await waitFor(() =>
      expect(result.current.states.a?.resourceToken).toBe("fresh"),
    );
    await act(async () => {
      finishInvalidation();
      await failure;
    });
    expect(result.current.states.a).toMatchObject({
      url: "http://edge-preview.localhost/fresh",
      resourceToken: "fresh",
      status: "ready",
    });
    expect(run).toHaveBeenCalledTimes(1);
    expect(api.releasePreviewResources).not.toHaveBeenCalledWith(["fresh"]);
  });

  it("revérifie les fichiers au retour de la fiche sans effacer l’image courante", async () => {
    const { result, rerender } = renderHook(
      ({ enabled }) => useAssetThumbnails(assets, "library", enabled, runJob),
      { initialProps: { enabled: true } },
    );
    act(() => result.current.setVisible("a", true));
    await waitFor(() => expect(result.current.states.a?.status).toBe("ready"));
    rerender({ enabled: false });
    expect(result.current.states.a?.url).toContain("/a");
    rerender({ enabled: true });
    await waitFor(() => expect(run).toHaveBeenCalledTimes(2));
  });
});
