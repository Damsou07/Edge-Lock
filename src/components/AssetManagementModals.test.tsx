import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import { api } from "../api";
import type { RunJob } from "../lib/useJobs";
import { RenameModal, VariantDeletionModal } from "./AssetManagementModals";

vi.mock("../api", () => ({
  api: { renameSnapshot: vi.fn(), renameVariant: vi.fn() },
}));

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.renameSnapshot).mockResolvedValue();
  vi.mocked(api.renameVariant).mockResolvedValue();
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
});

it("préremplit le nom et renomme uniquement les métadonnées de la version", async () => {
  const onSaved = vi.fn();
  render(
    <RenameModal
      target={{ kind: "snapshot", id: "s1", name: "Version initiale" }}
      onClose={vi.fn()}
      onSaved={onSaved}
    />,
  );
  const input = screen.getByRole("textbox", { name: "Nom de la version" });
  expect(input).toHaveValue("Version initiale");
  await userEvent.clear(input);
  await userEvent.type(input, "  Nouveau nom  ");
  await userEvent.click(screen.getByRole("button", { name: "Enregistrer" }));
  await waitFor(() => expect(onSaved).toHaveBeenCalledWith("Nouveau nom"));
  expect(api.renameSnapshot).toHaveBeenCalledExactlyOnceWith(
    "s1",
    "Nouveau nom",
  );
  expect(api.renameVariant).not.toHaveBeenCalled();
});

it("valide les noms UTF-8 et conserve le formulaire si le renommage échoue", async () => {
  const onSaved = vi.fn();
  vi.mocked(api.renameVariant).mockRejectedValue(
    new Error("Ce nom existe déjà."),
  );
  render(
    <RenameModal
      target={{ kind: "variant", id: "v1", name: "Principale" }}
      onClose={vi.fn()}
      onSaved={onSaved}
    />,
  );
  const input = screen.getByRole("textbox", { name: "Nom de la variante" });
  await userEvent.clear(input);
  expect(screen.getByRole("button", { name: "Enregistrer" })).toBeDisabled();
  await userEvent.type(input, "Désert");
  await userEvent.click(screen.getByRole("button", { name: "Enregistrer" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Ce nom existe déjà.",
  );
  expect(input).toHaveValue("Désert");
  expect(onSaved).not.toHaveBeenCalled();
});

const plan = {
  planToken: "frozen",
  variantId: "v1",
  variantName: "Désert",
  snapshotCount: 2,
  retainedSnapshotCount: 3,
  workspaceBytes: 2048,
  workspaceBytesComplete: true,
  warnings: [],
};
const report = {
  deletedAssetIds: [],
  deletedVariantIds: ["v1"],
  releasedBytes: 2048,
  cleanupPending: true,
  warnings: [],
};

it("analyse puis exige une confirmation avant la suppression de la variante", async () => {
  const run = vi.fn().mockResolvedValueOnce(plan).mockResolvedValueOnce(report);
  const onDeleted = vi.fn();
  render(
    <VariantDeletionModal
      variantId="v1"
      runJob={run as RunJob}
      onClose={vi.fn()}
      onDeleted={onDeleted}
    />,
  );
  await screen.findByText("Désert");
  expect(run).toHaveBeenCalledTimes(1);
  expect(screen.getByText(/3 versions partagées/)).toBeInTheDocument();
  await userEvent.click(
    screen.getByRole("button", { name: "Supprimer définitivement" }),
  );
  await waitFor(() => expect(onDeleted).toHaveBeenCalledWith(report));
  expect(run).toHaveBeenLastCalledWith({
    kind: "variantDeletionApply",
    variantId: "v1",
    planToken: "frozen",
  });
  expect(screen.getByRole("status")).toHaveTextContent(/nettoyage.*différé/);
});

it("permet une nouvelle analyse après un refus de jeton périmé", async () => {
  const run = vi
    .fn()
    .mockResolvedValueOnce(plan)
    .mockRejectedValueOnce(new Error("La variante a changé."))
    .mockResolvedValueOnce({ ...plan, planToken: "fresh" });
  render(
    <VariantDeletionModal
      variantId="v1"
      runJob={run as RunJob}
      onClose={vi.fn()}
      onDeleted={vi.fn()}
    />,
  );
  await screen.findByText("Désert");
  await userEvent.click(
    screen.getByRole("button", { name: "Supprimer définitivement" }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "La variante a changé.",
  );
  expect(
    screen.getByRole("button", { name: "Supprimer définitivement" }),
  ).toBeDisabled();
  await userEvent.click(
    screen.getByRole("button", { name: "Analyser à nouveau" }),
  );
  await waitFor(() => expect(run).toHaveBeenCalledTimes(3));
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "Supprimer définitivement" }),
    ).toBeEnabled(),
  );
});

it("attend la suppression soumise avant de fermer la confirmation", async () => {
  let finish!: (value: typeof report) => void;
  const run = vi
    .fn()
    .mockResolvedValueOnce(plan)
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
  const onClose = vi.fn();
  render(
    <VariantDeletionModal
      variantId="v1"
      runJob={run as RunJob}
      onClose={onClose}
      onDeleted={vi.fn()}
    />,
  );
  await screen.findByText("Désert");
  await userEvent.click(
    screen.getByRole("button", { name: "Supprimer définitivement" }),
  );
  expect(screen.getByRole("button", { name: "Annuler" })).toBeDisabled();
  expect(onClose).not.toHaveBeenCalled();
  await act(async () => finish(report));
  await userEvent.click(screen.getByText("Fermer", { selector: "button" }));
  expect(onClose).toHaveBeenCalledOnce();
});
