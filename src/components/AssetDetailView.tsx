import { lazy, Suspense, useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { api } from "../api";
import type {
  AppError,
  AssetDetail,
  ChangeSet,
  ExportConfig,
  Inspection,
  PreviewResult,
  Snapshot,
  Taxonomy,
  VariantDetail,
} from "../contracts";
import { defaultExportConfig } from "../contracts";
import {
  errorMessage,
  extension,
  formatBytes,
  formatCount,
  formatDate,
} from "../lib/format";
import type { RunJob } from "../lib/useJobs";
import { Icon } from "./Icon";
import { MetadataModal } from "./AssetForms";
import { Modal } from "./Modal";

const ModelViewer = lazy(() => import("./ModelViewer"));

function VersionModal({
  mode,
  snapshotId,
  variantId,
  runJob,
  onClose,
}: {
  mode: "snapshot" | "variant";
  snapshotId: string;
  variantId: string;
  runJob: RunJob;
  onClose: () => void;
}) {
  const [value, setValue] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      if (mode === "snapshot")
        await runJob({ kind: "snapshot", variantId, message: value.trim() });
      else await runJob({ kind: "variant", snapshotId, name: value.trim() });
      onClose();
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      title={mode === "snapshot" ? "Créer une version" : "Créer une variante"}
      subtitle={
        mode === "snapshot"
          ? "Un point de repère dans l’histoire de votre asset."
          : "Un dossier indépendant, à partir de cette version."
      }
      onClose={onClose}
    >
      <form className="form-stack" onSubmit={(event) => void submit(event)}>
        <label>
          {mode === "snapshot" ? "Qu’avez-vous changé ?" : "Nom de la variante"}
          <input
            autoFocus
            required
            maxLength={mode === "snapshot" ? 500 : 120}
            value={value}
            onChange={(event) => setValue(event.target.value)}
            placeholder={
              mode === "snapshot"
                ? "Ex. Nouvelles textures et ajustement du rig"
                : "Ex. Texture désert"
            }
          />
        </label>
        <div className="callout">
          <Icon name={mode === "snapshot" ? "save" : "branch"} />
          <p>
            {mode === "snapshot"
              ? "Enregistrez d’abord vos changements dans Blender. Edge Lock capture les fichiers présents sur le disque."
              : "Les fichiers de travail sont copiés dans un nouveau dossier. L’historique partage les fichiers identiques."}
          </p>
        </div>
        {error && (
          <div className="inline-error" role="alert">
            {error}
          </div>
        )}
        <footer className="modal-actions">
          <button type="button" className="button secondary" onClick={onClose}>
            {busy ? "Fermer" : "Annuler"}
          </button>
          <button className="button primary" disabled={busy || !value.trim()}>
            <Icon
              name={busy ? "refresh" : mode === "snapshot" ? "save" : "branch"}
              className={busy ? "spinning" : ""}
            />
            {busy
              ? "En cours…"
              : mode === "snapshot"
                ? "Créer la version"
                : "Créer la variante"}
          </button>
        </footer>
      </form>
    </Modal>
  );
}

export function AssetDetailView({
  id,
  taxonomy,
  generation,
  onBack,
  onChanged,
  onError,
  runJob,
}: {
  id: string;
  taxonomy: Taxonomy;
  generation: number;
  onBack: () => void;
  onChanged: () => void;
  onError: (message: string) => void;
  runJob: RunJob;
}) {
  const [detail, setDetail] = useState<AssetDetail | null>(null);
  const [variantId, setVariantId] = useState("");
  const [variantDetail, setVariantDetail] = useState<VariantDetail | null>(
    null,
  );
  const [snapshotId, setSnapshotId] = useState<string | null>(null);
  const [snapshotDetail, setSnapshotDetail] = useState<Snapshot | null>(null);
  const [snapshots, setSnapshots] = useState<Snapshot[]>([]);
  const [historyTotal, setHistoryTotal] = useState(0);
  const [historyBusy, setHistoryBusy] = useState(false);
  const [changes, setChanges] = useState<ChangeSet | null>(null);
  const [inspection, setInspection] = useState<Inspection | null>(null);
  const [preview, setPreview] = useState<PreviewResult | null>(null);
  const [previewBusy, setPreviewBusy] = useState(false);
  const [previewError, setPreviewError] = useState("");
  const [previewCandidates, setPreviewCandidates] = useState<string[]>([]);
  const [previewFile, setPreviewFile] = useState("");
  const [inspectionBusy, setInspectionBusy] = useState(false);
  const [changesBusy, setChangesBusy] = useState(false);
  const [externalChanged, setExternalChanged] = useState(false);
  const [config, setConfig] = useState<ExportConfig>(defaultExportConfig);
  const [configBusy, setConfigBusy] = useState(false);
  const [localRefresh, setLocalRefresh] = useState(0);
  const [tab, setTab] = useState<"files" | "history" | "changes">("files");
  const [fileLimit, setFileLimit] = useState(100);
  const [modal, setModal] = useState<
    "metadata" | "snapshot" | "variant" | null
  >(null);
  const [copyDone, setCopyDone] = useState(false);
  const [replacePath, setReplacePath] = useState<string | null>(null);
  const [exportBusy, setExportBusy] = useState(false);
  const selection = `${id}:${variantId}:${snapshotId ?? "current"}`;
  const currentSelection = useRef(selection);
  currentSelection.current = selection;
  const configEdited = useRef(false);
  const configVariant = useRef("");
  const editConfig = (update: (previous: ExportConfig) => ExportConfig) => {
    configEdited.current = true;
    setConfig(update);
  };

  useEffect(() => {
    let active = true;
    void api
      .getAsset(id)
      .then((value) => {
        if (active) {
          setDetail(value);
          setVariantId((previous) =>
            value.variants.some((variant) => variant.id === previous)
              ? previous
              : (value.variants[0]?.id ?? ""),
          );
        }
      })
      .catch((reason) => {
        if (active) onError(errorMessage(reason));
      });
    return () => {
      active = false;
    };
  }, [id, generation, localRefresh, onError]);
  useEffect(() => {
    if (!variantId) return;
    let active = true;
    void api
      .getVariant(variantId)
      .then((value) => {
        if (active) {
          setVariantDetail(value);
          if (
            !configEdited.current ||
            configVariant.current !== value.variant.id
          ) {
            setConfig(value.variant.config);
            configEdited.current = false;
            configVariant.current = value.variant.id;
          }
        }
      })
      .catch((reason) => {
        if (active) onError(errorMessage(reason));
      });
    return () => {
      active = false;
    };
  }, [variantId, generation, localRefresh, onError]);
  useEffect(() => {
    if (!variantId) return;
    let active = true;
    setHistoryBusy(true);
    void api
      .listSnapshots(variantId, 0)
      .then((value) => {
        if (active) {
          setSnapshots(value.items);
          setHistoryTotal(value.total);
        }
      })
      .catch((reason) => {
        if (active) onError(errorMessage(reason));
      })
      .finally(() => {
        if (active) setHistoryBusy(false);
      });
    return () => {
      active = false;
    };
  }, [variantId, generation, localRefresh, onError]);
  useEffect(() => {
    setSnapshotDetail(null);
    if (!snapshotId) return;
    let active = true;
    void api
      .getSnapshot(snapshotId)
      .then((value) => {
        if (active) setSnapshotDetail(value);
      })
      .catch((reason) => {
        if (active) onError(errorMessage(reason));
      });
    return () => {
      active = false;
    };
  }, [snapshotId, onError]);
  useEffect(() => {
    setPreview(null);
    setInspection(null);
    setPreviewBusy(false);
    setPreviewError("");
    setPreviewCandidates([]);
    setPreviewFile("");
    setInspectionBusy(false);
    setFileLimit(100);
  }, [selection]);
  useEffect(() => {
    if (!variantId) return;
    let active = true;
    setChangesBusy(true);
    setChanges(null);
    setExternalChanged(false);
    void runJob<ChangeSet>({ kind: "changes", variantId })
      .then((value) => {
        if (active) setChanges(value);
      })
      .catch((reason) => {
        if (active) onError(errorMessage(reason));
      })
      .finally(() => {
        if (active) setChangesBusy(false);
      });
    return () => {
      active = false;
    };
  }, [variantId, generation, localRefresh, runJob, onError]);
  useEffect(() => {
    let active = true;
    let unsubscribe: (() => void) | undefined;
    void api
      .onWorkspaceChanged((changedId) => {
        if (active && changedId === variantId) setExternalChanged(true);
      })
      .then((fn) => {
        if (active) unsubscribe = fn;
        else fn();
      })
      .catch(() => {
        /* Manual refresh is always available. */
      });
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, [variantId]);

  const selectedSnapshot =
    snapshotDetail?.id === snapshotId
      ? snapshotDetail
      : snapshots.find((snapshot) => snapshot.id === snapshotId);
  const files = snapshotId
    ? (selectedSnapshot?.entries ?? [])
    : (variantDetail?.files ?? []);
  const activeConfig = selectedSnapshot?.config ?? config;
  const configurationDirty =
    !snapshotId &&
    !!variantDetail &&
    (config.primaryFile !== variantDetail.variant.config.primaryFile ||
      config.scene !== variantDetail.variant.config.scene ||
      config.collection !== variantDetail.variant.config.collection ||
      config.applyModifiers !== variantDetail.variant.config.applyModifiers);
  const pendingCount = changes
    ? changes.added.length +
      changes.modified.length +
      changes.deleted.length +
      Number(changes.configChanged)
    : null;
  const dirty = externalChanged || (pendingCount ?? 0) > 0;
  const fileCandidates =
    variantDetail?.files.filter(
      (file) => file.kind === "file" && /\.(blend|glb)$/i.test(file.path),
    ) ?? [];
  const showFailure = (reason: unknown) => onError(errorMessage(reason));
  const sourceInspection =
    inspection ??
    (preview?.sourceKind === "generatedGlb" ? preview.inspection : null);
  const displayInspection = sourceInspection ?? preview?.inspection;
  const warnings = [
    ...new Set([
      ...(inspection?.warnings ?? []),
      ...(preview?.inspection.warnings ?? []),
    ]),
  ];
  const dependencies = [
    ...new Map(
      [
        ...(inspection?.dependencies ?? []),
        ...(preview?.inspection.dependencies ?? []),
      ]
        .filter(
          (dependency) =>
            dependency.renderRelevant !== false &&
            !["internal", "packed", "embedded", "ok"].includes(
              dependency.status,
            ),
        )
        .map((dependency) => [dependency.path, dependency]),
    ).values(),
  ];
  const previewDegraded = preview?.inspection.dependencies.some(
    (dependency) =>
      dependency.renderRelevant !== false && dependency.status === "missing",
  );

  const inspect = async () => {
    const target = selection;
    setInspectionBusy(true);
    try {
      const result = await runJob<Inspection>({
        kind: "inspect",
        variantId,
        snapshotId,
      });
      if (currentSelection.current === target) setInspection(result);
    } catch (reason) {
      showFailure(reason);
    } finally {
      if (currentSelection.current === target) setInspectionBusy(false);
    }
  };
  const loadPreviewFiles = async (target: string) => {
    if (snapshotId) return files;
    const updated = await api.getVariant(variantId);
    if (currentSelection.current !== target) return null;
    setVariantDetail(updated);
    return updated.files;
  };
  const generatePreview = async (selectedFile = previewFile) => {
    const target = selection;
    setPreviewBusy(true);
    setPreviewError("");
    setPreviewFile(selectedFile);
    try {
      if (selectedFile && !snapshotId) {
        const currentFiles = await loadPreviewFiles(target);
        if (!currentFiles) return;
        if (
          !currentFiles.some(
            (file) => file.kind === "file" && file.path === selectedFile,
          )
        ) {
          selectedFile = "";
          setPreviewFile("");
        }
      }
      const result = await runJob<PreviewResult>({
        kind: "preview",
        variantId,
        snapshotId,
        ...(selectedFile ? { previewFile: selectedFile } : {}),
      });
      if (currentSelection.current === target) {
        setPreview(result);
        if (result.sourceKind === "generatedGlb") setInspection(null);
        setPreviewCandidates([]);
        setExternalChanged(false);
      }
    } catch (reason) {
      if (currentSelection.current !== target) return;
      const error = reason as AppError | null;
      const candidates = error?.details?.candidates;
      if (
        error?.code === "preview_ambiguous" &&
        Array.isArray(candidates) &&
        candidates.length > 0 &&
        candidates.every((candidate) => typeof candidate === "string")
      ) {
        setPreviewCandidates(candidates);
        setPreviewFile("");
      } else {
        if (error?.code === "invalid" && selectedFile && !snapshotId)
          setPreviewFile("");
        setPreviewError(errorMessage(reason));
      }
    } finally {
      if (currentSelection.current === target) setPreviewBusy(false);
    }
  };
  const changePreviewFile = async () => {
    const target = selection;
    setPreviewBusy(true);
    setPreviewError("");
    try {
      const currentFiles = await loadPreviewFiles(target);
      if (!currentFiles) return;
      const candidates = currentFiles
        .filter((file) => file.kind === "file" && /\.glb$/i.test(file.path))
        .map((file) => file.path);
      setPreviewFile("");
      setPreviewCandidates(candidates);
      if (candidates.length === 0) await generatePreview("");
    } catch (reason) {
      if (currentSelection.current === target)
        setPreviewError(errorMessage(reason));
    } finally {
      if (currentSelection.current === target) setPreviewBusy(false);
    }
  };
  const refreshChanges = async () => {
    const targetId = variantId;
    setChangesBusy(true);
    try {
      const result = await runJob<ChangeSet>({ kind: "changes", variantId });
      if (!currentSelection.current.startsWith(`${id}:${targetId}:`)) return;
      const updated = await api.getVariant(targetId);
      if (currentSelection.current.startsWith(`${id}:${targetId}:`)) {
        setChanges(result);
        setVariantDetail(updated);
        setExternalChanged(false);
      }
    } catch (reason) {
      showFailure(reason);
    } finally {
      setChangesBusy(false);
    }
  };
  const saveConfig = async (event: FormEvent) => {
    event.preventDefault();
    setConfigBusy(true);
    try {
      await api.updateConfig(variantId, config);
      configEdited.current = false;
      setPreview(null);
      setInspection(null);
      setPreviewError("");
      setPreviewCandidates([]);
      setPreviewFile("");
      setLocalRefresh((value) => value + 1);
    } catch (reason) {
      showFailure(reason);
    } finally {
      setConfigBusy(false);
    }
  };
  const exportTo = async (destination: string, overwrite: boolean) => {
    setExportBusy(true);
    try {
      await runJob({
        kind: "export",
        variantId,
        snapshotId,
        destination,
        overwrite,
      });
      setReplacePath(null);
    } catch (reason) {
      if ((reason as AppError)?.code === "destination_exists")
        setReplacePath(destination);
      else showFailure(reason);
    } finally {
      setExportBusy(false);
    }
  };
  const exportGlb = async () => {
    try {
      const destination = await api.chooseExportPath(
        `${detail?.asset.name ?? "asset"}-${variantDetail?.variant.name ?? "principal"}.glb`,
      );
      if (destination) await exportTo(destination, false);
    } catch (reason) {
      showFailure(reason);
    }
  };
  const copyPath = async () => {
    if (!variantDetail) return;
    try {
      await api.copyText(variantDetail.variant.workspacePath);
      setCopyDone(true);
      setTimeout(() => setCopyDone(false), 1800);
    } catch (reason) {
      showFailure(reason);
    }
  };
  const loadMoreHistory = async () => {
    const target = variantId;
    setHistoryBusy(true);
    try {
      const next = await api.listSnapshots(target, snapshots.length);
      if (currentSelection.current.startsWith(`${id}:${target}:`)) {
        setSnapshots((previous) => [
          ...previous,
          ...next.items.filter(
            (item) => !previous.some((existing) => existing.id === item.id),
          ),
        ]);
        setHistoryTotal(next.total);
      }
    } catch (reason) {
      showFailure(reason);
    } finally {
      setHistoryBusy(false);
    }
  };

  if (!detail)
    return (
      <div className="page-loader">
        <Icon name="refresh" className="spinning" size={26} />
        <p>Ouverture de l’asset…</p>
      </div>
    );
  const asset = detail.asset;
  const variant = variantDetail?.variant;
  const category = taxonomy.categories.find(
    (item) => item.id === asset.categoryId,
  );
  const openSnapshot = (snapshot: Snapshot) => {
    setSnapshotId(snapshot.id);
    setTab("history");
  };

  return (
    <section className="detail-content">
      <button className="back-link" onClick={onBack}>
        <Icon name="back" size={16} />
        Retour à la bibliothèque
      </button>
      <div className="page-heading detail-heading">
        <div>
          <div className="asset-kicker">
            <span className="eyebrow">
              {category?.name ?? "SANS CATÉGORIE"}
            </span>
            {asset.archived && <span className="tag-chip">Archivé</span>}
          </div>
          <h1>{asset.name}</h1>
          <div className="detail-tags">
            {asset.tags.map((tag) => (
              <span className="tag-chip" key={tag}>
                <Icon name="tag" size={11} />
                {tag}
              </span>
            ))}
            {asset.gameIds.map((gameId) => (
              <span className="game-chip" key={gameId}>
                <Icon name="game" size={13} />
                {taxonomy.games.find((game) => game.id === gameId)?.name ??
                  "Projet"}
              </span>
            ))}
          </div>
        </div>
        <div className="heading-actions">
          <button
            className="button secondary"
            onClick={() => setModal("metadata")}
          >
            <Icon name="settings" />
            Organiser
          </button>
          <button
            className="button primary"
            disabled={!variant || exportBusy || configurationDirty}
            onClick={() => void exportGlb()}
          >
            <Icon
              name={exportBusy ? "refresh" : "export"}
              className={exportBusy ? "spinning" : ""}
            />
            Exporter .glb
          </button>
        </div>
      </div>
      <div className="variant-bar">
        <div className="variant-selector">
          <Icon name="branch" size={17} />
          <label htmlFor="variant-select">Variante</label>
          <select
            id="variant-select"
            value={variantId}
            onChange={(event) => {
              setVariantId(event.target.value);
              setSnapshotId(null);
              setVariantDetail(null);
              setSnapshots([]);
              configEdited.current = false;
            }}
          >
            {detail.variants.map((item) => (
              <option value={item.id} key={item.id}>
                {item.name}
              </option>
            ))}
          </select>
        </div>
        <button
          className="text-button"
          disabled={!variant}
          onClick={() => setModal("variant")}
        >
          <Icon name="plus" size={15} />
          Nouvelle variante
        </button>
        <span className="variant-bar-spacer" />
        <span className={`workspace-state ${dirty ? "dirty" : ""}`}>
          <span className="live-dot" />
          {snapshotId
            ? "Version enregistrée"
            : changesBusy
              ? "Vérification…"
              : dirty
                ? "Modifications non versionnées"
                : "À jour"}
        </span>
      </div>
      {snapshotId && (
        <div className="history-banner">
          <Icon name="history" />
          <div>
            <strong>
              Version du {formatDate(selectedSnapshot?.createdAt)}
            </strong>
            <span>{selectedSnapshot?.message}</span>
          </div>
          <button
            className="button secondary small-button"
            onClick={() => setSnapshotId(null)}
          >
            Revenir au travail en cours
          </button>
        </div>
      )}
      <div className="asset-workbench">
        <div className="preview-column">
          <div className="preview-panel">
            {preview ? (
              <Suspense
                fallback={
                  <div className="preview-loading">
                    <Icon name="refresh" className="spinning" />
                    Préparation de la visionneuse…
                  </div>
                }
              >
                <ModelViewer
                  url={preview.url}
                  label={
                    snapshotId ? "VERSION ENREGISTRÉE" : "DOSSIER DE TRAVAIL"
                  }
                />
              </Suspense>
            ) : (
              <div className="preview-placeholder">
                <div className="preview-grid" />
                <div className="preview-object">
                  <Icon name="cube" size={82} />
                </div>
                <h2>Votre asset, sous tous les angles.</h2>
                <p>
                  {previewBusy
                    ? "Préparation de l’aperçu local…"
                    : "Explorez le modèle et ses animations sans quitter votre bibliothèque."}
                </p>
                <button
                  className="button secondary"
                  disabled={
                    !variant ||
                    previewBusy ||
                    configurationDirty ||
                    previewCandidates.length > 0
                  }
                  onClick={() => void generatePreview()}
                >
                  <Icon
                    name={previewBusy ? "refresh" : "play"}
                    className={previewBusy ? "spinning" : ""}
                  />
                  {previewBusy
                    ? "Génération en cours…"
                    : "Afficher l’aperçu 3D"}
                </button>
                <span className="preview-format">
                  GLB · ROTATION · ZOOM · ANIMATIONS
                </span>
              </div>
            )}
            {preview && (
              <div className="preview-caption">
                <span title={preview.sourceFile}>
                  {externalChanged
                    ? "Le dossier a changé : actualisez l’aperçu."
                    : `${preview.sourceLabel} · ${preview.sourceFile}`}
                </span>
                {preview.sourceKind === "assetGlb" && (
                  <button
                    className="text-button"
                    disabled={previewBusy || configurationDirty}
                    onClick={() => void changePreviewFile()}
                  >
                    Changer le GLB
                  </button>
                )}
                <button
                  className="text-button"
                  disabled={previewBusy || configurationDirty}
                  onClick={() => void generatePreview()}
                >
                  <Icon
                    name="refresh"
                    size={13}
                    className={previewBusy ? "spinning" : ""}
                  />
                  Actualiser
                </button>
              </div>
            )}
          </div>
          {previewCandidates.length > 0 && (
            <form
              className="preview-source-choice"
              onSubmit={(event) => {
                event.preventDefault();
                void generatePreview();
              }}
            >
              <label>
                GLB de prévisualisation
                <select
                  value={previewFile}
                  disabled={previewBusy}
                  onChange={(event) => setPreviewFile(event.target.value)}
                >
                  <option value="">Choisir un fichier GLB</option>
                  {previewCandidates.map((candidate) => (
                    <option key={candidate} value={candidate}>
                      {candidate}
                    </option>
                  ))}
                </select>
              </label>
              <button
                className="button secondary small-button"
                disabled={!previewFile || previewBusy || configurationDirty}
              >
                Utiliser ce GLB
              </button>
            </form>
          )}
          {previewError && (
            <div className="inline-error preview-notice" role="alert">
              <strong>Aperçu indisponible</strong>
              <p>{previewError}</p>
              <p>
                Le fichier source reste accessible depuis l’espace de travail.
              </p>
            </div>
          )}
          {previewDegraded && (
            <div className="preview-notice preview-degraded" role="status">
              <Icon name="warning" size={16} />
              <span>
                Aperçu dégradé : certaines ressources de rendu sont absentes.
                Consultez la section Dépendances.
              </span>
            </div>
          )}
          <div className="asset-metrics">
            <div>
              <Icon name="database" />
              <span>{snapshotId ? "Cette version" : "Dossier de travail"}</span>
              <strong>
                {formatBytes(
                  snapshotId
                    ? selectedSnapshot?.logicalBytes
                    : (changes?.workingBytes ?? variant?.workingBytes),
                )}
              </strong>
            </div>
            <div>
              <Icon name="history" />
              <span title="Stockage de l’historique de toutes les variantes">
                Historique
              </span>
              <strong>{formatBytes(asset.historyBytes)}</strong>
            </div>
            <div>
              <Icon name="mesh" />
              <span>Triangles</span>
              <strong>
                {formatCount(
                  displayInspection?.geometry.triangles ??
                    (snapshotId ? null : variant?.triangles),
                )}
              </strong>
            </div>
            <div>
              <Icon name="cube" />
              <span>Objets maillés</span>
              <strong>
                {formatCount(displayInspection?.geometry.meshObjects)}
              </strong>
            </div>
          </div>
          {displayInspection && (
            <div className="inspection-note">
              <span>
                {formatCount(displayInspection.geometry.vertices)} sommets
              </span>
              <span
                title={
                  displayInspection.geometry.edges == null
                    ? "Le nombre d’arêtes n’est pas disponible pour cette analyse."
                    : undefined
                }
              >
                {formatCount(displayInspection.geometry.edges)} arêtes
              </span>
              <span>{formatCount(displayInspection.geometry.faces)} faces</span>
              <span>
                {displayInspection.blenderVersion.startsWith("GLB")
                  ? displayInspection.blenderVersion
                  : `Blender ${displayInspection.blenderVersion}`}
              </span>
              <span
                className="inspection-provenance"
                title={
                  sourceInspection
                    ? "Dernière analyse de la source ; utilisez Analyser pour l’actualiser."
                    : undefined
                }
              >
                {sourceInspection
                  ? "Analyse de la source"
                  : "Géométrie de l’aperçu"}
                {" · "}
                {inspection
                  ? (selectedSnapshot?.config.primaryFile ??
                    variantDetail?.variant.config.primaryFile)
                  : preview?.sourceFile}
              </span>
            </div>
          )}
        </div>
        <aside className="workspace-panel">
          <div className="panel-heading">
            <h2>Espace de travail</h2>
            <Icon name="folder" size={17} />
          </div>
          <p className="muted small">
            Un dossier réel pour Blender et vos agents IA.
          </p>
          <div className="workspace-path">
            <code>{variant?.workspacePath ?? "Chargement…"}</code>
            <button
              className="icon-button"
              aria-label="Copier le chemin du dossier"
              title="Copier le chemin"
              disabled={!variant}
              onClick={() => void copyPath()}
            >
              <Icon name={copyDone ? "check" : "copy"} size={16} />
            </button>
          </div>
          <div className="workspace-actions">
            <button
              className="button secondary"
              disabled={!variant}
              onClick={() =>
                void api.openWorkspace(variantId).catch(showFailure)
              }
            >
              <Icon name="folder" />
              Ouvrir le dossier
            </button>
            <button
              className="button blender-button"
              disabled={!variant || !!snapshotId || configurationDirty}
              title={
                snapshotId
                  ? "Créez une variante pour ouvrir cette version dans Blender."
                  : "Ouvrir le fichier principal"
              }
              onClick={() => void api.openBlender(variantId).catch(showFailure)}
            >
              <Icon name="open" />
              Ouvrir dans Blender
            </button>
          </div>
          <div className="panel-divider" />
          <form
            className="config-form"
            onSubmit={(event) => void saveConfig(event)}
          >
            <div className="panel-heading">
              <h3>Source de l’asset</h3>
              <span className="format-badge">
                {extension(activeConfig.primaryFile)}
              </span>
            </div>
            <label>
              Fichier principal
              <select
                value={activeConfig.primaryFile ?? ""}
                disabled={!!snapshotId}
                onChange={(event) =>
                  editConfig((previous) => ({
                    ...previous,
                    primaryFile: event.target.value || null,
                    scene: null,
                    collection: null,
                  }))
                }
              >
                <option value="">Détection automatique</option>
                {fileCandidates.map((file) => (
                  <option value={file.path} key={file.path}>
                    {file.path}
                  </option>
                ))}
                {activeConfig.primaryFile &&
                  !fileCandidates.some(
                    (file) => file.path === activeConfig.primaryFile,
                  ) && (
                    <option value={activeConfig.primaryFile}>
                      {activeConfig.primaryFile}
                    </option>
                  )}
              </select>
            </label>
            <div className="config-fields">
              <label>
                Scène
                {sourceInspection ? (
                  <select
                    disabled={!!snapshotId}
                    value={activeConfig.scene ?? ""}
                    onChange={(event) =>
                      editConfig((previous) => ({
                        ...previous,
                        scene: event.target.value || null,
                        collection: null,
                      }))
                    }
                  >
                    <option value="">Scène active</option>
                    {sourceInspection.scenes.map((scene) => (
                      <option key={scene} value={scene}>
                        {scene}
                      </option>
                    ))}
                  </select>
                ) : (
                  <input
                    disabled={!!snapshotId}
                    value={activeConfig.scene ?? ""}
                    placeholder="Scène active"
                    onChange={(event) =>
                      editConfig((previous) => ({
                        ...previous,
                        scene: event.target.value || null,
                      }))
                    }
                  />
                )}
              </label>
              <label>
                Collection
                {sourceInspection ? (
                  <select
                    disabled={!!snapshotId}
                    value={activeConfig.collection ?? ""}
                    onChange={(event) =>
                      editConfig((previous) => ({
                        ...previous,
                        collection: event.target.value || null,
                      }))
                    }
                  >
                    <option value="">Scène entière</option>
                    {sourceInspection.collections.map((collection) => (
                      <option key={collection} value={collection}>
                        {collection}
                      </option>
                    ))}
                  </select>
                ) : (
                  <input
                    disabled={!!snapshotId}
                    value={activeConfig.collection ?? ""}
                    placeholder="Scène entière"
                    onChange={(event) =>
                      editConfig((previous) => ({
                        ...previous,
                        collection: event.target.value || null,
                      }))
                    }
                  />
                )}
              </label>
            </div>
            <label className="checkbox-label">
              <input
                disabled={!!snapshotId}
                type="checkbox"
                checked={activeConfig.applyModifiers}
                onChange={(event) =>
                  editConfig((previous) => ({
                    ...previous,
                    applyModifiers: event.target.checked,
                  }))
                }
              />
              Appliquer les modificateurs à l’export
            </label>
            <div className="config-actions">
              <button
                type="button"
                className="text-button"
                disabled={!variant || inspectionBusy || configurationDirty}
                onClick={() => void inspect()}
              >
                <Icon
                  name="refresh"
                  size={13}
                  className={inspectionBusy ? "spinning" : ""}
                />
                {inspectionBusy ? "Analyse…" : "Analyser"}
              </button>
              <button
                className="button secondary small-button"
                disabled={!variant || !!snapshotId || configBusy}
              >
                {configBusy ? "Enregistrement…" : "Enregistrer"}
              </button>
            </div>
            {configurationDirty && (
              <p className="field-help">
                Enregistrez la source pour l’utiliser dans Blender, les versions
                et les exports.
              </p>
            )}
          </form>
          <div className="panel-divider" />
          <div className="version-callout">
            <Icon name="history" size={21} />
            <div>
              <strong>Gardez une trace de vos avancées.</strong>
              <p>Enregistrez dans Blender, puis créez une version ici.</p>
            </div>
          </div>
          <button
            className="button primary full-width"
            disabled={!variant || !!snapshotId || configurationDirty}
            onClick={() => setModal("snapshot")}
          >
            <Icon name="save" />
            Créer une version
          </button>
        </aside>
      </div>
      {warnings.length > 0 && (
        <section
          className="inspection-warnings"
          aria-label="Informations d’analyse"
        >
          <Icon name="warning" />
          <div>
            <h2>Informations d’analyse</h2>
            {warnings.map((warning) => (
              <p key={warning}>{warning}</p>
            ))}
          </div>
        </section>
      )}
      {dependencies.length > 0 && (
        <section className="inspection-warnings" aria-label="Dépendances">
          <Icon name="warning" />
          <div>
            <h2>Dépendances</h2>
            {dependencies.map((dependency) => (
              <p key={dependency.path}>
                <code>{dependency.path}</code> —{" "}
                {dependency.status === "missing"
                  ? "manquante"
                  : dependency.status === "external"
                    ? "externe"
                    : dependency.status}
              </p>
            ))}
          </div>
        </section>
      )}
      <div className="asset-bottom-panel">
        <div
          className="asset-tabs"
          role="tablist"
          aria-label="Contenu de l’asset"
        >
          <button
            role="tab"
            aria-selected={tab === "files"}
            className={tab === "files" ? "active" : ""}
            onClick={() => setTab("files")}
          >
            <Icon name="folder" size={16} />
            Fichiers
            <span>{files.filter((file) => file.kind === "file").length}</span>
          </button>
          <button
            role="tab"
            aria-selected={tab === "history"}
            className={tab === "history" ? "active" : ""}
            onClick={() => setTab("history")}
          >
            <Icon name="history" size={16} />
            Historique<span>{historyTotal}</span>
          </button>
          <button
            role="tab"
            aria-selected={tab === "changes"}
            className={tab === "changes" ? "active" : ""}
            onClick={() => setTab("changes")}
          >
            <Icon name="branch" size={16} />
            Modifications
            {pendingCount != null && (
              <span className={pendingCount > 0 ? "changes-count" : ""}>
                {pendingCount}
              </span>
            )}
          </button>
          <span className="tab-spacer" />
          <button
            className="text-button refresh-files"
            disabled={changesBusy}
            onClick={() => void refreshChanges()}
          >
            <Icon
              name="refresh"
              size={14}
              className={changesBusy ? "spinning" : ""}
            />
            Actualiser
          </button>
        </div>
        {tab === "files" && (
          <div role="tabpanel" className="file-panel">
            {files.length ? (
              <>
                <div className="file-table-header">
                  <span>NOM</span>
                  <span>TYPE</span>
                  <span>TAILLE</span>
                </div>
                {files.slice(0, fileLimit).map((file) => (
                  <div className="file-row" key={file.path}>
                    <span>
                      <Icon
                        name={file.kind === "directory" ? "folder" : "file"}
                        size={17}
                      />
                      <code>{file.path}</code>
                      {file.path === activeConfig.primaryFile && (
                        <span className="primary-file-badge">Principal</span>
                      )}
                    </span>
                    <span>
                      {file.kind === "directory"
                        ? "Dossier"
                        : extension(file.path)}
                    </span>
                    <span>
                      {file.kind === "directory" ? "—" : formatBytes(file.size)}
                    </span>
                  </div>
                ))}
                {files.length > fileLimit && (
                  <button
                    className="button secondary load-more"
                    onClick={() => setFileLimit((value) => value + 100)}
                  >
                    Afficher les 100 suivants
                  </button>
                )}
              </>
            ) : (
              <div className="small-empty">
                <Icon name="folder" size={24} />
                <p>Ce dossier est prêt à accueillir vos fichiers.</p>
              </div>
            )}
          </div>
        )}
        {tab === "history" && (
          <div role="tabpanel" className="history-panel">
            {snapshots.length ? (
              snapshots.map((snapshot, index) => (
                <div
                  className={`history-row ${snapshotId === snapshot.id ? "selected" : ""}`}
                  key={snapshot.id}
                >
                  <div className="timeline-point">
                    <span />
                  </div>
                  <div className="history-description">
                    <div>
                      <strong>{snapshot.message}</strong>
                      {index === 0 && (
                        <span className="head-badge">Dernière version</span>
                      )}
                    </div>
                    <span>
                      {formatDate(snapshot.createdAt)}
                      <i>·</i>
                      {formatBytes(snapshot.logicalBytes)}
                      <i>·</i>
                      <code>{snapshot.id.slice(0, 8)}</code>
                    </span>
                  </div>
                  <button
                    className="button secondary small-button"
                    onClick={() => openSnapshot(snapshot)}
                  >
                    Consulter
                    <Icon name="chevron" size={13} />
                  </button>
                </div>
              ))
            ) : (
              <div className="small-empty">
                <Icon name="history" size={24} />
                <p>
                  {historyBusy
                    ? "Chargement de l’historique…"
                    : "Aucune version enregistrée."}
                </p>
              </div>
            )}
            {snapshots.length < historyTotal && (
              <button
                className="button secondary load-more"
                disabled={historyBusy}
                onClick={() => void loadMoreHistory()}
              >
                {historyBusy
                  ? "Chargement…"
                  : "Charger les versions précédentes"}
              </button>
            )}
          </div>
        )}
        {tab === "changes" && (
          <div role="tabpanel" className="changes-panel">
            <p className="changes-description">
              Comparaison du dossier de travail avec sa dernière version
              enregistrée.
              {externalChanged &&
                " De nouvelles modifications ont été détectées ; actualisez la liste."}
            </p>
            {changes && pendingCount === 0 ? (
              <div className="small-empty">
                <Icon name="check" size={25} />
                <p>
                  Tout est à jour. Votre dernière version correspond aux
                  fichiers sur le disque.
                </p>
              </div>
            ) : changes ? (
              <>
                {changes.configChanged && (
                  <div className="change-row modified">
                    <span>Modifié</span>
                    <code>Configuration de l’asset</code>
                  </div>
                )}
                {(["added", "modified", "deleted"] as const).flatMap((kind) =>
                  changes[kind].map((path) => (
                    <div
                      className={`change-row ${kind}`}
                      key={`${kind}-${path}`}
                    >
                      <span>
                        {kind === "added"
                          ? "Ajouté"
                          : kind === "deleted"
                            ? "Supprimé"
                            : "Modifié"}
                      </span>
                      <code>{path}</code>
                    </div>
                  )),
                )}
              </>
            ) : (
              <div className="small-empty">
                <Icon
                  name="refresh"
                  size={24}
                  className={changesBusy ? "spinning" : ""}
                />
                <p>
                  {changesBusy
                    ? "Analyse des fichiers…"
                    : "Actualisez pour comparer les fichiers."}
                </p>
              </div>
            )}
          </div>
        )}
      </div>
      {modal === "metadata" && (
        <MetadataModal
          asset={asset}
          taxonomy={taxonomy}
          onClose={() => setModal(null)}
          onSaved={onChanged}
        />
      )}
      {(modal === "snapshot" || modal === "variant") && variant && (
        <VersionModal
          mode={modal}
          variantId={variantId}
          snapshotId={snapshotId ?? variant.headSnapshotId}
          runJob={runJob}
          onClose={() => setModal(null)}
        />
      )}
      {replacePath && (
        <Modal
          title="Remplacer le fichier existant ?"
          onClose={() => setReplacePath(null)}
        >
          <div className="form-stack">
            <p>
              Un fichier existe déjà à cet emplacement. Le remplacement sera
              effectué uniquement après un export réussi.
            </p>
            <code className="path-text">{replacePath}</code>
            <footer className="modal-actions">
              <button
                className="button secondary"
                onClick={() => setReplacePath(null)}
              >
                Annuler
              </button>
              <button
                className="button primary"
                disabled={exportBusy}
                onClick={() => void exportTo(replacePath, true)}
              >
                Remplacer et exporter
              </button>
            </footer>
          </div>
        </Modal>
      )}
    </section>
  );
}
