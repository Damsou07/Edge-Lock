import { describe, expect, it } from "vitest";
import { formatBytes, formatCount, errorMessage } from "./format";

describe("format des informations de bibliothèque", () => {
  it("affiche les tailles sans perdre les petits fichiers", () => {
    expect(formatBytes(0)).toBe("0 o");
    expect(formatBytes(1024)).toBe("1 Ko");
    expect(formatBytes(1536)).toBe("1,5 Ko");
    expect(formatBytes(1024 ** 3)).toBe("1 Go");
  });
  it("distingue une mesure absente de zéro", () => {
    expect(formatCount(null)).toBe("—");
    expect(formatCount(0)).toBe("0");
  });
  it("rend les erreurs IPC structurées compréhensibles", () => {
    expect(
      errorMessage({ code: "io", message: "Le dossier est verrouillé." }),
    ).toBe("Le dossier est verrouillé.");
    expect(errorMessage(new Error("Disque plein"))).toBe("Disque plein");
  });
});
