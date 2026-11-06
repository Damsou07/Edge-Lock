import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import { api } from "../api";
import { defaultExportConfig } from "../contracts";
import type { Snapshot, Variant, WorkspaceDiscardReport } from "../contracts";
import type { RunJob } from "../lib/useJobs";
import { WorkspaceDiscardModal } from "./WorkspaceDiscardModal";

vi.mock("../api", () => ({ api: { getSnapshot: vi.fn() } }));

const variant: Variant = {
  id: "variant",
  assetId: "asset",
  name: "Principale",
  workspacePath: "C:\\Vault\\workspace",
  headSnapshotId: "head",
  workingBytes: 20,
  triangles: 12,
  config: defaultExportConfig,
};
const snapshot: Snapshot = {
  id: "head",
  variantId: "variant",
  parentId: null,
  createdAt: "2026-11-04T10:00:00Z",
  message: "Textures validées",
  logicalBytes: 10,
  config: defaultExportConfig,
  entries: [],
};
const report: WorkspaceDiscardReport = {
  variantId: "variant",
  headSnapshotId: "head",
  pending: false,
  reconfirmationRequired: false,
  cleanupPending: false,
  warnings: [],
};
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getSnapshot).mockResolvedValue(snapshot);
});

it("nomme la dernière version et attend la confirmation explicite", async () => {
  const run = vi.fn().mockResolvedValue(report);
  const onClose = vi.fn();
  const onDiscarded = vi.fn();
  render(
    <WorkspaceDiscardModal
      variant={variant}
      runJob={run as RunJob}
      onClose={onClose}
      onDiscarded={onDiscarded}
    />,
  );
  await screen.findByText("Textures validées");
  expect(run).not.toHaveBeenCalled();
  const confirm = screen.getByRole("button", {
    name: "Supprimer les modifications",
  });
  expect(confirm).toBeDisabled();
  await userEvent.click(screen.getByRole("checkbox"));
  await userEvent.click(confirm);
  await waitFor(() =>
    expect(run).toHaveBeenCalledWith({
      kind: "discardChanges",
      variantId: "variant",
      expectedHead: "head",
    }),
  );
  expect(onDiscarded).toHaveBeenCalledWith(report);
  expect(onClose).toHaveBeenCalledTimes(1);
});

it("annule sans lancer de tâche", async () => {
  const run = vi.fn();
  const onClose = vi.fn();
  render(
    <WorkspaceDiscardModal
      variant={variant}
      runJob={run as RunJob}
      onClose={onClose}
      onDiscarded={vi.fn()}
    />,
  );
  await screen.findByText("Textures validées");
  await userEvent.click(
    screen.getByRole("button", { name: "Conserver mes modifications" }),
  );
  expect(onClose).toHaveBeenCalledTimes(1);
  expect(run).not.toHaveBeenCalled();
});

it("empêche les doubles confirmations et permet de réessayer après une erreur", async () => {
  let reject!: (reason: Error) => void;
  const run = vi
    .fn()
    .mockImplementationOnce(
      () =>
        new Promise((_, fail) => {
          reject = fail;
        }),
    )
    .mockResolvedValue(report);
  const onClose = vi.fn();
  render(
    <WorkspaceDiscardModal
      variant={variant}
      runJob={run as RunJob}
      onClose={onClose}
      onDiscarded={vi.fn()}
    />,
  );
  await screen.findByText("Textures validées");
  await userEvent.click(screen.getByRole("checkbox"));
  await userEvent.click(
    screen.getByRole("button", { name: "Supprimer les modifications" }),
  );
  expect(
    screen.getByRole("button", { name: "Annulation des modifications…" }),
  ).toBeDisabled();
  await userEvent.click(screen.getByRole("button", { name: "Fermer" }));
  expect(onClose).not.toHaveBeenCalled();
  await act(async () => reject(new Error("Fichier verrouillé")));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Fichier verrouillé",
  );
  await userEvent.click(
    screen.getByRole("button", { name: "Supprimer les modifications" }),
  );
  await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  expect(run).toHaveBeenCalledTimes(2);
});

it("affiche le nettoyage différé après une annulation réussie", async () => {
  const result = {
    ...report,
    cleanupPending: true,
    warnings: ["Un fichier reste verrouillé."],
  };
  const onDiscarded = vi.fn();
  const onClose = vi.fn();
  render(
    <WorkspaceDiscardModal
      variant={variant}
      runJob={vi.fn().mockResolvedValue(result) as RunJob}
      onClose={onClose}
      onDiscarded={onDiscarded}
    />,
  );
  await screen.findByText("Textures validées");
  await userEvent.click(screen.getByRole("checkbox"));
  await userEvent.click(
    screen.getByRole("button", { name: "Supprimer les modifications" }),
  );
  expect(await screen.findByRole("status")).toHaveTextContent(
    "Modifications annulées",
  );
  expect(screen.getByText("Un fichier reste verrouillé.")).toBeInTheDocument();
  expect(onDiscarded).toHaveBeenCalledWith(result);
  expect(onClose).not.toHaveBeenCalled();
  expect(
    screen.queryByRole("button", { name: "Supprimer les modifications" }),
  ).not.toBeInTheDocument();
});

it.each([
  [{ pending: true }, "Demande d’annulation enregistrée"],
  [{ reconfirmationRequired: true }, "Confirmez de nouveau"],
])(
  "distingue une demande qui n’a pas restauré les fichiers",
  async (state, message) => {
    const onDiscarded = vi.fn();
    const onClose = vi.fn();
    render(
      <WorkspaceDiscardModal
        variant={variant}
        runJob={vi.fn().mockResolvedValue({ ...report, ...state }) as RunJob}
        onClose={onClose}
        onDiscarded={onDiscarded}
      />,
    );
    await screen.findByText("Textures validées");
    await userEvent.click(screen.getByRole("checkbox"));
    await userEvent.click(
      screen.getByRole("button", { name: "Supprimer les modifications" }),
    );
    expect(await screen.findByRole("status")).toHaveTextContent(message);
    expect(onDiscarded).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    await userEvent.click(screen.getByText("Fermer", { selector: "button" }));
    expect(onClose).toHaveBeenCalledOnce();
  },
);

it("refuse de confirmer si la version cible ne peut pas être chargée", async () => {
  vi.mocked(api.getSnapshot).mockRejectedValue(
    new Error("Version indisponible"),
  );
  const run = vi.fn();
  render(
    <WorkspaceDiscardModal
      variant={variant}
      runJob={run as RunJob}
      onClose={vi.fn()}
      onDiscarded={vi.fn()}
    />,
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Version indisponible",
  );
  await userEvent.click(screen.getByRole("checkbox"));
  expect(
    screen.getByRole("button", { name: "Supprimer les modifications" }),
  ).toBeDisabled();
  expect(run).not.toHaveBeenCalled();
});
