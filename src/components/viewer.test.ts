import { describe, expect, it } from "vitest";
import { inspectGlb, previewCacheKey } from "./viewer-utils";

function glb(json: unknown): ArrayBuffer {
  const source = new TextEncoder().encode(JSON.stringify(json));
  const length = Math.ceil(source.length / 4) * 4;
  const bytes = new Uint8Array(20 + length);
  const view = new DataView(bytes.buffer);
  view.setUint32(0, 0x46546c67, true);
  view.setUint32(4, 2, true);
  view.setUint32(8, bytes.length, true);
  view.setUint32(12, length, true);
  view.setUint32(16, 0x4e4f534a, true);
  bytes.fill(32, 20);
  bytes.set(source, 20);
  return bytes.buffer;
}

describe("validation GLB avant chargement", () => {
  it("refuse une image SVG embarquée par bufferView", () => {
    expect(() =>
      inspectGlb(
        glb({
          asset: { version: "2.0" },
          images: [{ bufferView: 0, mimeType: "image/svg+xml" }],
        }),
      ),
    ).toThrow(/Format d’image/);
  });
  it("accepte des métadonnées JSON dépassant 16 Mio", () => {
    expect(() =>
      inspectGlb(
        glb({
          asset: { version: "2.0" },
          extras: { notes: "x".repeat(16 * 1024 * 1024) },
        }),
      ),
    ).not.toThrow();
  });
  it("accepte les ressources embarquées et les noms des clips", () => {
    expect(
      inspectGlb(
        glb({
          asset: { version: "2.0" },
          buffers: [{ byteLength: 0 }],
          animations: [{ name: "Marche" }],
        }),
      ).animationNames,
    ).toEqual(["Marche"]);
  });
  it.each([
    "https://example.com/a.png",
    "../secret.png",
    "file:///C:/secret.png",
    "data:image/png;base64,AAA",
  ])("refuse toute ressource externe %s", (uri) => {
    expect(() =>
      inspectGlb(glb({ asset: { version: "2.0" }, images: [{ uri }] })),
    ).toThrow(/embarquées/);
  });
  it.each(["png", "jpeg", "webp"])(
    "accepte une image %s intégrée en base64",
    (format) => {
      expect(() =>
        inspectGlb(
          glb({
            asset: { version: "2.0" },
            images: [{ uri: `data:image/${format};base64,AAAA` }],
          }),
        ),
      ).not.toThrow();
    },
  );
  it("refuse une image SVG intégrée et les buffers data URI", () => {
    expect(() =>
      inspectGlb(
        glb({
          asset: { version: "2.0" },
          images: [{ uri: "data:image/svg+xml;base64,AAAA" }],
        }),
      ),
    ).toThrow(/embarquées/);
    expect(() =>
      inspectGlb(
        glb({
          asset: { version: "2.0" },
          buffers: [{ uri: "data:application/octet-stream;base64,AAAA" }],
        }),
      ),
    ).toThrow(/embarquées/);
  });
  it("refuse un fichier tronqué avant le parseur Three", () => {
    expect(() => inspectGlb(new ArrayBuffer(4))).toThrow(/GLB/);
  });
  it("refuse les buffers externes et les structures inattendues", () => {
    expect(() =>
      inspectGlb(
        glb({
          asset: { version: "2.0" },
          buffers: [{ uri: "https://example.com/data.bin" }],
        }),
      ),
    ).toThrow(/embarquées/);
    expect(() => inspectGlb(glb(null))).toThrow(/métadonnées GLB/);
    expect(() =>
      inspectGlb(glb({ asset: { version: "2.0" }, images: {} })),
    ).toThrow(/métadonnées GLB/);
  });
  it("explique les extensions exigeant un décodeur indisponible", () => {
    expect(() =>
      inspectGlb(
        glb({
          asset: { version: "2.0" },
          extensionsRequired: ["KHR_draco_mesh_compression"],
        }),
      ),
    ).toThrow(/Compression KHR_draco_mesh_compression/);
  });
  it("invalide la vue lorsque sa révision change", () => {
    expect(previewCacheKey("variant-a", "revision-a")).not.toBe(
      previewCacheKey("variant-a", "revision-b"),
    );
  });
});
