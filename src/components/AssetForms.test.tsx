import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../api";
import type { Asset, Taxonomy } from "../contracts";
import { MetadataModal } from "./AssetForms";

vi.mock("../api", () => ({ api: { updateAsset: vi.fn() } }));

const asset: Asset = {
  id: "asset-one",
  name: "Robot",
  categoryId: null,
  tags: ["robot"],
  gameIds: ["first"],
  archived: false,
  createdAt: "2026-11-01",
  updatedAt: "2026-11-01",
  workingBytes: 0,
  historyBytes: 0,
  triangles: null,
  variantCount: 1,
};
const taxonomy: Taxonomy = {
  categories: [],
  games: [
    { id: "first", name: "Premier jeu" },
    { id: "second", name: "Second jeu" },
  ],
  tags: [],
};

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.updateAsset).mockResolvedValue(undefined);
});

describe("organisation d’un asset", () => {
  it("permet de modifier les projets et l’archivage au clavier puis conserve les autres métadonnées", async () => {
    const user = userEvent.setup();
    const onSaved = vi.fn();
    const onClose = vi.fn();
    render(
      <MetadataModal
        asset={asset}
        taxonomy={taxonomy}
        onSaved={onSaved}
        onClose={onClose}
      />,
    );

    const first = screen.getByRole("checkbox", { name: "Premier jeu" });
    const second = screen.getByRole("checkbox", { name: "Second jeu" });
    const archived = screen.getByRole("checkbox", {
      name: /Archiver cet asset/,
    });
    expect(first).toBeChecked();
    expect(second).not.toBeChecked();
    expect(archived).not.toBeChecked();

    await user.click(screen.getByRole("textbox", { name: /Tags/ }));
    await user.tab();
    expect(first).toHaveFocus();
    await user.keyboard("[Space]");
    await user.tab();
    expect(second).toHaveFocus();
    await user.keyboard("[Space]");
    await user.tab();
    expect(archived).toHaveFocus();
    await user.keyboard("[Space]");
    expect(first).not.toBeChecked();
    expect(second).toBeChecked();
    expect(archived).toBeChecked();

    await user.click(screen.getByRole("button", { name: "Enregistrer" }));
    await waitFor(() =>
      expect(api.updateAsset).toHaveBeenCalledWith({
        id: "asset-one",
        name: "Robot",
        categoryId: null,
        tags: ["robot"],
        gameIds: ["second"],
        archived: true,
      }),
    );
    expect(onSaved).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("active les cases par leurs libellés et permet de restaurer un asset archivé", async () => {
    const user = userEvent.setup();
    render(
      <MetadataModal
        asset={{ ...asset, archived: true }}
        taxonomy={taxonomy}
        onSaved={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    await user.click(screen.getByText("Second jeu"));
    await user.click(screen.getByText(/Archiver cet asset/));
    expect(screen.getByRole("checkbox", { name: "Second jeu" })).toBeChecked();
    expect(
      screen.getByRole("checkbox", { name: /Archiver cet asset/ }),
    ).not.toBeChecked();
    await user.click(screen.getByRole("button", { name: "Enregistrer" }));
    await waitFor(() =>
      expect(api.updateAsset).toHaveBeenCalledWith(
        expect.objectContaining({
          gameIds: ["first", "second"],
          archived: false,
        }),
      ),
    );
  });
});
