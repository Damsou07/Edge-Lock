export const MAX_PREVIEW_BYTES = 256 * 1024 * 1024;

type GlbDocument = {
  asset?: { version?: string };
  images?: { uri?: unknown }[];
  buffers?: { uri?: unknown }[];
  extensionsRequired?: string[];
  animations?: { name?: string }[];
};

export function isEmbeddedImageUri(value: unknown): value is string {
  if (
    typeof value !== "string" ||
    !/^data:image\/(png|jpeg|webp);base64,[A-Za-z0-9+/]+={0,2}$/.test(value)
  )
    return false;
  return value.slice(value.indexOf(",") + 1).length % 4 === 0;
}

export function inspectGlb(buffer: ArrayBuffer): { animationNames: string[] } {
  if (buffer.byteLength > MAX_PREVIEW_BYTES)
    throw new Error("Cet aperçu dépasse 256 Mo. Ouvrez l’asset dans Blender.");
  if (buffer.byteLength < 20) throw new Error("Fichier GLB incomplet.");
  const view = new DataView(buffer);
  if (
    view.getUint32(0, true) !== 0x46546c67 ||
    view.getUint32(4, true) !== 2 ||
    view.getUint32(8, true) !== buffer.byteLength
  )
    throw new Error("Le fichier n’est pas un GLB 2.0 valide.");
  const jsonLength = view.getUint32(12, true);
  if (
    view.getUint32(16, true) !== 0x4e4f534a ||
    jsonLength > buffer.byteLength - 20 ||
    jsonLength > 16 * 1024 * 1024
  )
    throw new Error("Structure GLB invalide.");
  let document: GlbDocument;
  try {
    document = JSON.parse(
      new TextDecoder().decode(new Uint8Array(buffer, 20, jsonLength)),
    ) as GlbDocument;
  } catch {
    throw new Error("Les métadonnées GLB sont illisibles.");
  }
  if (!document || typeof document !== "object" || Array.isArray(document))
    throw new Error("Les métadonnées GLB sont invalides.");
  for (const entries of [
    document.images,
    document.buffers,
    document.animations,
  ]) {
    if (
      entries !== undefined &&
      (!Array.isArray(entries) ||
        entries.some(
          (entry) =>
            !entry || typeof entry !== "object" || Array.isArray(entry),
        ))
    )
      throw new Error("Les métadonnées GLB sont invalides.");
  }
  if (
    document.extensionsRequired !== undefined &&
    (!Array.isArray(document.extensionsRequired) ||
      document.extensionsRequired.some((value) => typeof value !== "string"))
  )
    throw new Error("Les métadonnées GLB sont invalides.");
  if (document.asset?.version !== "2.0")
    throw new Error("Version GLB non prise en charge.");
  if (
    (document.buffers ?? []).some((resource) => resource.uri !== undefined) ||
    (document.images ?? []).some(
      (resource) =>
        resource.uri !== undefined && !isEmbeddedImageUri(resource.uri),
    )
  )
    throw new Error(
      "L’aperçu exige des textures et ressources embarquées dans le GLB. Régénérez-le depuis Blender.",
    );
  const unsupported = document.extensionsRequired?.find((name) =>
    [
      "KHR_draco_mesh_compression",
      "EXT_meshopt_compression",
      "KHR_meshopt_compression",
      "KHR_texture_basisu",
    ].includes(name),
  );
  if (unsupported)
    throw new Error(
      `Compression ${unsupported} non prise en charge par l’aperçu. Utilisez un GLB sans compression.`,
    );
  return {
    animationNames: (document.animations ?? []).map(
      (animation, index) => animation.name || `Animation ${index + 1}`,
    ),
  };
}

export function previewCacheKey(variantId: string, revision: string): string {
  return `${variantId}:${revision}`;
}
