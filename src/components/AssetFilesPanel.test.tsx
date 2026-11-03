import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { ManifestEntry } from "../contracts";
import { AssetFilesPanel } from "./AssetFilesPanel";

const entries: ManifestEntry[] = [
  { path: "main.BLEND", kind: "file", hash: "a", size: 1024 },
  { path: "main.glb", kind: "file", hash: "b", size: 2048 },
  { path: "textures/iris.PNG", kind: "file", hash: "c", size: 3072 },
  { path: "textures/albedo.png", kind: "file", hash: "d", size: 4096 },
  { path: "references.v2/README", kind: "file", hash: "e", size: 0 },
  { path: "textures", kind: "directory", hash: null, size: 0 },
];

describe("filtres des fichiers", () => {
  it("regroupe les extensions sans tenir compte de la casse et conserve le fichier principal", async () => {
    render(<AssetFilesPanel files={entries} primaryFile="main.BLEND" />);
    expect(screen.getByText("Principal")).toBeInTheDocument();
    await userEvent.selectOptions(
      screen.getByLabelText("Type de fichier"),
      "PNG",
    );
    expect(screen.getByText("textures/iris.PNG")).toBeInTheDocument();
    expect(screen.getByText("textures/albedo.png")).toBeInTheDocument();
    expect(screen.queryByText("main.BLEND")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("2 fichiers sur 5");
  });

  it("distingue dossiers et fichiers sans extension même si leur dossier contient un point", async () => {
    render(<AssetFilesPanel files={entries} primaryFile={null} />);
    const type = screen.getByLabelText("Type de fichier");
    await userEvent.selectOptions(type, "none");
    expect(screen.getByText("references.v2/README")).toBeInTheDocument();
    expect(screen.queryByText("main.BLEND")).not.toBeInTheDocument();
    await userEvent.selectOptions(type, "directory");
    expect(screen.getByText("textures")).toBeInTheDocument();
    expect(screen.queryByText("references.v2/README")).not.toBeInTheDocument();
  });

  it("combine type et bornes inclusives avec une unité en base 1024", async () => {
    render(<AssetFilesPanel files={entries} primaryFile={null} />);
    await userEvent.selectOptions(
      screen.getByLabelText("Unité de taille"),
      "1024",
    );
    await userEvent.type(screen.getByLabelText("Taille minimum"), "3");
    await userEvent.type(screen.getByLabelText("Taille maximum"), "4");
    await userEvent.selectOptions(
      screen.getByLabelText("Type de fichier"),
      "PNG",
    );
    expect(screen.getByText("textures/iris.PNG")).toBeInTheDocument();
    expect(screen.getByText("textures/albedo.png")).toBeInTheDocument();
    expect(
      screen.queryByText("textures", { exact: true }),
    ).not.toBeInTheDocument();
    await userEvent.clear(screen.getByLabelText("Taille maximum"));
    await userEvent.type(screen.getByLabelText("Taille maximum"), "3");
    expect(screen.queryByText("textures/albedo.png")).not.toBeInTheDocument();
    expect(screen.getByText("textures/iris.PNG")).toBeInTheDocument();
  });

  it("signale une plage invalide et permet de réinitialiser sans modifier les fichiers", async () => {
    render(<AssetFilesPanel files={entries} primaryFile={null} />);
    await userEvent.type(screen.getByLabelText("Taille minimum"), "4");
    await userEvent.type(screen.getByLabelText("Taille maximum"), "2");
    expect(screen.getByRole("alert")).toHaveTextContent(
      "minimum ne peut pas dépasser le maximum",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Réinitialiser les filtres" }),
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByText("main.BLEND")).toBeInTheDocument();
    expect(screen.getByLabelText("Taille minimum")).toHaveValue(null);
    expect(screen.getByLabelText("Type de fichier")).toHaveValue("all");
  });

  it("refuse les tailles négatives et accepte les bornes décimales", async () => {
    render(<AssetFilesPanel files={entries} primaryFile={null} />);
    const minimum = screen.getByLabelText("Taille minimum");
    await userEvent.type(minimum, "-1");
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Saisissez des tailles positives ou nulles.",
    );
    expect(minimum).toHaveAttribute("aria-invalid", "true");
    await userEvent.clear(minimum);
    await userEvent.selectOptions(
      screen.getByLabelText("Unité de taille"),
      "1024",
    );
    await userEvent.type(minimum, "1.5");
    await userEvent.type(screen.getByLabelText("Taille maximum"), "2");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByText("main.glb")).toBeInTheDocument();
    expect(screen.queryByText("main.BLEND")).not.toBeInTheDocument();
    expect(screen.queryByText("textures/iris.PNG")).not.toBeInTheDocument();
  });

  it("distingue un filtre sans résultat d’un dossier vide et accepte la taille zéro", async () => {
    const { rerender } = render(
      <AssetFilesPanel files={entries} primaryFile={null} />,
    );
    await userEvent.type(screen.getByLabelText("Taille maximum"), "0");
    expect(screen.getByText("references.v2/README")).toBeInTheDocument();
    expect(
      screen.queryByText("textures", { exact: true }),
    ).not.toBeInTheDocument();
    await userEvent.selectOptions(
      screen.getByLabelText("Type de fichier"),
      "BLEND",
    );
    expect(
      screen.getByText("Aucun fichier ne correspond aux filtres."),
    ).toBeInTheDocument();
    rerender(<AssetFilesPanel files={[]} primaryFile={null} />);
    expect(
      screen.getByText("Ce dossier est prêt à accueillir vos fichiers."),
    ).toBeInTheDocument();
  });

  it("filtre avant la pagination et revient aux 100 premiers résultats après un changement de filtre", async () => {
    const many: ManifestEntry[] = Array.from({ length: 205 }, (_, i) => ({
      path: `report-${i}.txt`,
      kind: "file",
      hash: String(i),
      size: i,
    }));
    const { rerender } = render(
      <AssetFilesPanel files={[...many, ...entries]} primaryFile={null} />,
    );
    expect(screen.queryByText("main.BLEND")).not.toBeInTheDocument();
    await userEvent.selectOptions(
      screen.getByLabelText("Type de fichier"),
      "BLEND",
    );
    expect(screen.getByText("main.BLEND")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Afficher les 100 suivants" }),
    ).not.toBeInTheDocument();
    await userEvent.selectOptions(
      screen.getByLabelText("Type de fichier"),
      "TXT",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Afficher les 100 suivants" }),
    );
    expect(screen.getByText("report-150.txt")).toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Taille minimum"), "1");
    expect(screen.queryByText("report-150.txt")).not.toBeInTheDocument();
    rerender(
      <AssetFilesPanel files={[...many, ...entries]} primaryFile={null} />,
    );
    expect(screen.getByLabelText("Type de fichier")).toHaveValue("TXT");
    expect(screen.getByLabelText("Taille minimum")).toHaveValue(1);
  });
});
