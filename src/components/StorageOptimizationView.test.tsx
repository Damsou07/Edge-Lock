import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { Job, JobRequest, StorageDiagnostic } from "../contracts";
import type { RunJob, RunJobOptions } from "../lib/useJobs";
import { StorageOptimizationView } from "./StorageOptimizationView";

const report: StorageDiagnostic = {
  startedAt: "2026-11-03T08:00:00Z",
  completedAt: "2026-11-03T08:00:01Z",
  complete: true,
  total: { fileCount: 4, bytes: 400 },
  logicalObjectBytes: 800,
  categories: [{ key: "objects", label: "Objets", fileCount: 4, bytes: 400 }],
  details: [
    {
      key: "purgeCandidates",
      label: "Candidats connus sans référence",
      count: 1,
      bytes: 100,
      examples: [],
    },
    {
      key: "unreferencedObjects",
      label: "Objets sans référence à examiner",
      count: 1,
      bytes: 50,
      examples: [{ path: "objects/ab/orphan", size: 50 }],
    },
  ],
  purgeAvailable: true,
  purgeBlockedReason: null,
  warnings: [],
};
const plan = {
  planToken: "global-plan",
  logicalBytes: 800,
  physicalBytes: 400,
  reclaimableBytes: 100,
  releasedBytes: 0,
  versions: [],
  warnings: [],
};

function view(job: ReturnType<typeof vi.fn>, jobs: Job[] = []) {
  return (
    <StorageOptimizationView
      libraryRoot="C:/Vault"
      jobs={jobs}
      runJob={job as RunJob}
    />
  );
}

describe("optimisation du stockage global", () => {
  it("attend une demande avant l’inventaire et distingue les orphelins de la purge", async () => {
    const job = vi.fn().mockResolvedValue(report);
    render(view(job));
    expect(job).not.toHaveBeenCalled();
    await userEvent.click(
      screen.getByRole("button", { name: "Lancer le diagnostic" }),
    );
    await screen.findByRole("table", {
      name: "Répartition du stockage de la bibliothèque",
    });
    expect(job).toHaveBeenCalledWith(
      { kind: "storageDiagnostic" },
      { signal: expect.any(AbortSignal) },
    );
    expect(screen.getByText("Sans référence, à examiner")).toBeInTheDocument();
    expect(screen.getByText("Non déclaré supprimable")).toBeInTheDocument();
    expect(
      screen.getByText(/Le contenu des fichiers n’a pas été vérifié/),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByText("Objets sans référence à examiner"));
    expect(screen.getByText("objects/ab/orphan")).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Actualiser le diagnostic" }),
    ).toBeEnabled();
  });

  it("signale les résultats partiels et bloque une purge indisponible", async () => {
    const job = vi.fn().mockResolvedValue({
      ...report,
      complete: false,
      purgeAvailable: false,
      purgeBlockedReason: "Restauration à récupérer",
      warnings: ["Dossier inaccessible"],
    });
    render(view(job));
    await userEvent.click(
      screen.getByRole("button", { name: "Lancer le diagnostic" }),
    );
    await screen.findByText(/Rapport partiel/);
    expect(
      screen.getByText("Purge indisponible : Restauration à récupérer"),
    ).toBeInTheDocument();
    expect(screen.getByText("Dossier inaccessible")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Libérer les objets inutilisés" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Compresser avec Zstd" }),
    ).toBeEnabled();
  });

  it("annule et ignore un résultat arrivé après l’annulation", async () => {
    let finish!: (value: StorageDiagnostic) => void;
    const job = vi.fn<
      (
        request: JobRequest,
        options?: RunJobOptions,
      ) => Promise<StorageDiagnostic>
    >(
      () =>
        new Promise<StorageDiagnostic>((resolve) => {
          finish = resolve;
        }),
    );
    render(view(job));
    await userEvent.click(
      screen.getByRole("button", { name: "Lancer le diagnostic" }),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Annuler le diagnostic" }),
    );
    expect(job.mock.calls[0]?.[1]?.signal?.aborted).toBe(true);
    await act(async () => finish(report));
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
    expect(screen.getByText(/Diagnostic annulé/)).toBeInTheDocument();
  });

  it("annule en quittant la page et n’affiche pas le rapport d’une autre bibliothèque", async () => {
    let finish!: (value: StorageDiagnostic) => void;
    const job = vi.fn<
      (
        request: JobRequest,
        options?: RunJobOptions,
      ) => Promise<StorageDiagnostic>
    >(
      () =>
        new Promise<StorageDiagnostic>((resolve) => {
          finish = resolve;
        }),
    );
    const current = render(
      <StorageOptimizationView
        key="old"
        libraryRoot="C:/Old"
        jobs={[]}
        runJob={job as RunJob}
      />,
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Lancer le diagnostic" }),
    );
    current.rerender(
      <StorageOptimizationView
        key="new"
        libraryRoot="C:/New"
        jobs={[]}
        runJob={job as RunJob}
      />,
    );
    expect(job.mock.calls[0]?.[1]?.signal?.aborted).toBe(true);
    await act(async () => finish(report));
    expect(screen.getByText("C:/New")).toBeInTheDocument();
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
    expect(job).toHaveBeenCalledTimes(1);
  });

  it("confirme une action globale sans variante puis marque le rapport à actualiser", async () => {
    const job = vi.fn(async (request: JobRequest) =>
      request.kind === "storageDiagnostic"
        ? report
        : request.kind === "storageApply"
          ? { ...plan, releasedBytes: 100 }
          : plan,
    );
    render(view(job));
    await userEvent.click(
      screen.getByRole("button", { name: "Lancer le diagnostic" }),
    );
    await screen.findByRole("table");
    await userEvent.click(
      screen.getByRole("button", { name: "Compresser avec Zstd" }),
    );
    await screen.findByText("Espace récupérable : 100 o");
    expect(screen.getAllByText("Portée : toute la bibliothèque")).toHaveLength(
      3,
    );
    expect(job).toHaveBeenCalledTimes(2);
    await userEvent.click(screen.getByRole("button", { name: "Confirmer" }));
    await waitFor(() =>
      expect(screen.getByText(/Stockage modifié/)).toBeInTheDocument(),
    );
    expect(job).toHaveBeenLastCalledWith({
      kind: "storageApply",
      variantId: undefined,
      action: "recompress",
      snapshotId: undefined,
      planToken: "global-plan",
    });
  });

  it("affiche la progression puis une erreur récupérable", async () => {
    let fail!: (error: Error) => void;
    const job = vi.fn(
      () =>
        new Promise<StorageDiagnostic>((_resolve, reject) => {
          fail = reject;
        }),
    );
    const running: Job = {
      id: "scan",
      kind: "storageDiagnostic",
      status: "running",
      phase: "Inventaire des objets",
      completed: 42,
      total: null,
      result: null,
      error: null,
    };
    render(view(job, [running]));
    await userEvent.click(
      screen.getByRole("button", { name: "Lancer le diagnostic" }),
    );
    expect(screen.getByText("Inventaire des objets")).toBeInTheDocument();
    expect(screen.getByText(/42 fichiers ou éléments/)).toBeInTheDocument();
    await act(async () => fail(new Error("Catalogue indisponible")));
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Catalogue indisponible",
    );
    expect(
      screen.getByRole("button", { name: "Lancer le diagnostic" }),
    ).toBeEnabled();
  });
});
