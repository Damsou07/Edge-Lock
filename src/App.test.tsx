import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { api } from "./api";
import type { Asset, Bootstrap, Job, WorkspaceStatus } from "./contracts";

vi.mock("./api", () => ({
  api: {
    bootstrap: vi.fn(),
    chooseFolder: vi.fn(),
    openLibrary: vi.fn(),
    getTaxonomy: vi.fn(),
    listAssets: vi.fn(),
    onJob: vi.fn(),
    onWorkspaceChanged: vi.fn(),
    onWorkspaceStatus: vi.fn(),
    getWorkspaceStatus: vi.fn(),
    requestWorkspaceSync: vi.fn(),
    onFolderDrop: vi.fn(),
    getAsset: vi.fn(),
    startJob: vi.fn(),
    getJob: vi.fn(),
    cancelJob: vi.fn(),
    getAssetThumbnails: vi.fn(),
    releasePreviewResources: vi.fn(),
    invalidateThumbnail: vi.fn(),
  },
}));

const startup: Bootstrap = {
  settings: { libraryPath: null, blenderPath: null },
  library: null,
  startupError: null,
  version: "0.1.0",
};
const sample: Asset = {
  id: "asset-one",
  name: "Robot explorateur",
  categoryId: null,
  tags: ["robot"],
  gameIds: [],
  archived: false,
  createdAt: "2026-11-01",
  updatedAt: "2026-11-01",
  workingBytes: 1024,
  historyBytes: 1024,
  triangles: 120,
  variantCount: 2,
};
const openedLibrary: Bootstrap = {
  ...startup,
  library: {
    root: "C:\\Vault",
    assetCount: 1,
    physicalBytes: 2048,
    recoveryNotices: [],
  },
};
let dropFolders: ((paths: string[]) => void) | undefined;

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.bootstrap).mockResolvedValue(startup);
  vi.mocked(api.onJob).mockResolvedValue(() => {});
  vi.mocked(api.onWorkspaceChanged).mockResolvedValue(() => {});
  vi.mocked(api.onWorkspaceStatus).mockResolvedValue(() => {});
  dropFolders = undefined;
  vi.mocked(api.onFolderDrop).mockImplementation(async (callback) => {
    dropFolders = callback;
    return () => {};
  });
  vi.mocked(api.getTaxonomy).mockResolvedValue({
    categories: [],
    games: [],
    tags: [],
  });
  vi.mocked(api.listAssets).mockResolvedValue({ items: [], total: 0 });
  vi.mocked(api.getAssetThumbnails).mockResolvedValue({});
  vi.mocked(api.releasePreviewResources).mockResolvedValue(undefined);
  vi.mocked(api.cancelJob).mockResolvedValue(undefined);
  // Visibility itself is exercised by the dedicated thumbnail queue tests.
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
});

describe("bibliothèque Edge Lock", () => {
  it("ouvre l’aide sans bibliothèque et retrouve le contexte après fermeture", async () => {
    vi.spyOn(HTMLDialogElement.prototype, "showModal").mockImplementation(
      function (this: HTMLDialogElement) {
        this.setAttribute("open", "");
      },
    );
    vi.spyOn(HTMLDialogElement.prototype, "close").mockImplementation(function (
      this: HTMLDialogElement,
    ) {
      this.removeAttribute("open");
    });
    render(<App />);
    await screen.findByRole("button", { name: "Choisir un dossier" });
    await userEvent.click(
      screen.getByRole("button", { name: "Aide et mode d’emploi" }),
    );
    const dialog = screen.getByRole("dialog", {
      name: "Aide et mode d’emploi",
    });
    expect(
      within(dialog).getByRole("heading", { name: /1 \.blend \+ 1 \.glb/ }),
    ).toBeInTheDocument();
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Fermer" }),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Choisir un dossier" }),
    ).toBeInTheDocument();
    expect(api.startJob).not.toHaveBeenCalled();
  });
  it("désactive le menu d’optimisation sans bibliothèque ouverte", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "Choisir un dossier" });
    expect(
      screen.getByRole("button", { name: "Optimisation du stockage" }),
    ).toBeDisabled();
  });
  it("ouvre le stockage global dans une bibliothèque vide et conserve une navigation active unique", async () => {
    vi.mocked(api.bootstrap).mockResolvedValue({
      ...openedLibrary,
      library: { ...openedLibrary.library!, assetCount: 0 },
    });
    render(<App />);
    const menu = await screen.findByRole("button", {
      name: "Optimisation du stockage",
    });
    await waitFor(() => expect(menu).toBeEnabled());
    await userEvent.click(menu);
    expect(
      await screen.findByRole("heading", {
        name: "Optimisation du stockage",
        level: 1,
      }),
    ).toBeInTheDocument();
    expect(menu).toHaveClass("active");
    expect(
      screen.getByRole("button", { name: /Tous les assets/ }),
    ).not.toHaveClass("active");
    expect(
      screen.getByRole("button", { name: "Compresser avec Zstd" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "Libérer les objets inutilisés" }),
    ).toBeEnabled();
    expect(api.startJob).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Archives" }));
    expect(
      await screen.findByRole("heading", { name: /Archives/, level: 1 }),
    ).toBeInTheDocument();
    expect(menu).not.toHaveClass("active");
  });
  it("actualise le catalogue sur un nouvel artefact et ignore les événements anciens ou répétés", async () => {
    let emit!: (status: WorkspaceStatus) => void;
    vi.mocked(api.bootstrap).mockResolvedValue(openedLibrary);
    vi.mocked(api.listAssets).mockResolvedValue({ items: [sample], total: 1 });
    vi.mocked(api.onWorkspaceStatus).mockImplementation(async (callback) => {
      emit = callback;
      return () => {};
    });
    render(<App />);
    await screen.findByRole("heading", { name: "Robot explorateur" });
    const initialReads = vi.mocked(api.listAssets).mock.calls.length;
    const ready: WorkspaceStatus = {
      variantId: "one",
      revision: 1,
      state: "ready",
      sourceChanged: false,
      canCreateSnapshot: false,
      artifactKey: "first",
      error: null,
    };
    await act(async () => emit(ready));
    await waitFor(() =>
      expect(api.listAssets).toHaveBeenCalledTimes(initialReads + 1),
    );
    await act(async () => emit({ ...ready, revision: 2 }));
    expect(api.listAssets).toHaveBeenCalledTimes(initialReads + 1);
    await act(async () =>
      emit({ ...ready, revision: 4, artifactKey: "second" }),
    );
    await waitFor(() =>
      expect(api.listAssets).toHaveBeenCalledTimes(initialReads + 2),
    );
    await act(async () => emit({ ...ready, revision: 3 }));
    expect(api.listAssets).toHaveBeenCalledTimes(initialReads + 2);
  });
  it("respecte la navigation choisie pendant l’annulation d’une vignette avant ouverture", async () => {
    vi.mocked(api.bootstrap).mockResolvedValue(openedLibrary);
    vi.mocked(api.listAssets).mockImplementation(async (filter) =>
      filter.archived ? { items: [], total: 0 } : { items: [sample], total: 1 },
    );
    vi.mocked(api.getAsset).mockResolvedValue({ asset: sample, variants: [] });
    let showCard!: () => void;
    vi.stubGlobal(
      "IntersectionObserver",
      class {
        constructor(
          callback: (entries: { isIntersecting: boolean }[]) => void,
        ) {
          showCard = () => callback([{ isIntersecting: true }]);
        }
        observe() {}
        disconnect() {}
      },
    );
    vi.mocked(api.startJob).mockResolvedValue("thumbnail-pending");
    let finish!: (job: Job) => void;
    vi.mocked(api.getJob).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    render(<App />);
    const card = await screen.findByRole("button", {
      name: /Robot explorateur/,
    });
    act(() => showCard());
    await waitFor(() =>
      expect(api.getJob).toHaveBeenCalledWith("thumbnail-pending"),
    );
    await userEvent.click(card);
    await waitFor(() =>
      expect(api.cancelJob).toHaveBeenCalledWith("thumbnail-pending"),
    );
    expect(api.getAsset).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Archives" }));
    await waitFor(() =>
      expect(api.listAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ archived: true }),
      ),
    );
    await act(async () =>
      finish({
        id: "thumbnail-pending",
        kind: "thumbnail",
        status: "cancelled",
        phase: "Annulé",
        completed: 0,
        total: null,
        result: null,
        error: null,
      }),
    );
    expect(api.getAsset).not.toHaveBeenCalled();
    expect(
      screen.getByRole("heading", { name: /^Archives/ }),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("Retour à la bibliothèque"),
    ).not.toBeInTheDocument();
  });

  it("affiche une image fixe en cache dans la carte, sans visionneuse 3D", async () => {
    vi.mocked(api.bootstrap).mockResolvedValue(openedLibrary);
    vi.mocked(api.listAssets).mockResolvedValue({ items: [sample], total: 1 });
    vi.mocked(api.getAssetThumbnails).mockResolvedValue({
      [sample.id]: {
        url: "http://edge-preview.localhost/image",
        resourceToken: "image",
      },
    });
    const { container } = render(<App />);
    const image = await screen.findByAltText(
      "Aperçu texturé de Robot explorateur",
    );
    expect(image).toHaveAttribute("src", "http://edge-preview.localhost/image");
    expect(container.querySelector("canvas")).toBeNull();
    expect(api.startJob).not.toHaveBeenCalled();
  });
  it("demande un emplacement réel et respecte l’annulation du sélecteur", async () => {
    vi.mocked(api.chooseFolder).mockResolvedValue(null);
    render(<App />);
    await userEvent.click(
      await screen.findByRole("button", { name: "Choisir un dossier" }),
    );
    expect(api.openLibrary).not.toHaveBeenCalled();
    expect(
      screen.getByRole("heading", { name: /Vos créations,/ }),
    ).toBeInTheDocument();
  });
  it("affiche les assets retournés et envoie la recherche au backend", async () => {
    vi.mocked(api.bootstrap).mockResolvedValue({
      ...startup,
      library: {
        root: "C:\\Vault",
        assetCount: 1,
        physicalBytes: 2048,
        recoveryNotices: [],
      },
    });
    vi.mocked(api.listAssets).mockResolvedValue({ items: [sample], total: 1 });
    render(<App />);
    expect(await screen.findByText("Robot explorateur")).toBeInTheDocument();
    await userEvent.type(
      screen.getByRole("searchbox", { name: "Rechercher des assets" }),
      "robot",
    );
    await waitFor(() =>
      expect(api.listAssets).toHaveBeenLastCalledWith(
        expect.objectContaining({ search: "robot", offset: 0 }),
      ),
    );
  });
  it("affiche une erreur de démarrage sans remplacer la bibliothèque", async () => {
    vi.mocked(api.bootstrap).mockResolvedValue({
      ...startup,
      startupError: "Bibliothèque verrouillée par une autre instance.",
    });
    render(<App />);
    expect(
      await screen.findByText(
        "Bibliothèque verrouillée par une autre instance.",
      ),
    ).toBeInTheDocument();
    expect(api.openLibrary).not.toHaveBeenCalled();
  });
  it("affiche l’espace disque réel de la bibliothèque", async () => {
    vi.mocked(api.bootstrap).mockResolvedValue(openedLibrary);
    render(<App />);
    expect(await screen.findByText("2 Ko sur disque")).toBeInTheDocument();
    expect(screen.getByText("Total de la bibliothèque")).toBeInTheDocument();
  });
  it("préremplit un dossier déposé et attend la confirmation avant l’import", async () => {
    vi.mocked(api.bootstrap).mockResolvedValue(openedLibrary);
    vi.mocked(api.startJob).mockResolvedValue("import-one");
    vi.mocked(api.getJob).mockResolvedValue({
      id: "import-one",
      kind: "import",
      status: "completed",
      phase: "Terminé",
      completed: 1,
      total: 1,
      result: null,
      error: null,
    });
    render(<App />);
    await waitFor(() => expect(dropFolders).toBeDefined());
    act(() => dropFolders?.(["C:\\Créations\\Robot"]));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    expect(screen.getByLabelText("Dossier source")).toHaveValue(
      "C:\\Créations\\Robot",
    );
    expect(screen.getByLabelText("Nom de l’asset")).toHaveValue("Robot");
    expect(api.startJob).not.toHaveBeenCalled();
    await userEvent.click(
      screen.getByRole("button", { name: "Importer le dossier" }),
    );
    await waitFor(() =>
      expect(api.startJob).toHaveBeenCalledWith(
        expect.objectContaining({
          kind: "import",
          source: "C:\\Créations\\Robot",
          name: "Robot",
        }),
      ),
    );
  });
  it("refuse plusieurs dossiers déposés sans lancer d’import", async () => {
    vi.mocked(api.bootstrap).mockResolvedValue(openedLibrary);
    render(<App />);
    await waitFor(() => expect(dropFolders).toBeDefined());
    act(() => dropFolders?.(["C:\\Robot", "C:\\Rocher"]));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Déposez un seul dossier à la fois.",
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(api.startJob).not.toHaveBeenCalled();
  });
  it("préserve un formulaire d’import en cours lors d’un nouveau dépôt", async () => {
    vi.mocked(api.bootstrap).mockResolvedValue(openedLibrary);
    render(<App />);
    await userEvent.click(
      await screen.findByRole("button", { name: "Importer" }),
    );
    await userEvent.type(screen.getByLabelText("Dossier source"), "C:\\Manuel");
    await userEvent.type(screen.getByLabelText("Nom de l’asset"), "Mon asset");
    act(() => dropFolders?.(["C:\\Robot"]));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Fermez le formulaire ouvert",
    );
    expect(screen.getByLabelText("Dossier source")).toHaveValue("C:\\Manuel");
    expect(screen.getByLabelText("Nom de l’asset")).toHaveValue("Mon asset");
    expect(api.startJob).not.toHaveBeenCalled();
  });
  it("n’écoute pas les dépôts sans bibliothèque ouverte", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "Choisir un dossier" });
    expect(api.onFolderDrop).not.toHaveBeenCalled();
  });
});
