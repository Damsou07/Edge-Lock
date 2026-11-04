import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HelpModal } from "./HelpModal";

beforeEach(() => {
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

describe("mode d’emploi", () => {
  it("explique les sources multiples et les suppressions confirmées", () => {
    render(<HelpModal onClose={vi.fn()} />);
    const dialog = screen.getByRole("dialog", {
      name: "Aide et mode d’emploi",
    });
    expect(
      within(dialog).getByRole("heading", {
        name: "Plusieurs modèles par workspace",
      }),
    ).toBeInTheDocument();
    expect(dialog).toHaveTextContent("au moins un .blend");
    expect(dialog).toHaveTextContent("Vider les archives");
    expect(dialog).toHaveTextContent("Re-détecter automatiquement");
    expect(dialog).toHaveTextContent("sous-dossiers compris");
    expect(dialog).toHaveTextContent(".blend1");
    expect(
      within(dialog).getByRole("heading", {
        name: "Démarrer et travailler dans Blender",
      }),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByRole("heading", {
        name: "Versions, variantes et sauvegardes",
      }),
    ).toBeInTheDocument();
  });

  it("ferme par le bouton ou Échap", async () => {
    const onClose = vi.fn();
    render(<HelpModal onClose={onClose} />);
    await userEvent.click(screen.getByRole("button", { name: "Fermer" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    fireEvent(
      screen.getByRole("dialog"),
      new Event("cancel", { bubbles: false, cancelable: true }),
    );
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});
