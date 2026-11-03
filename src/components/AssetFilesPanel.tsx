import { useId, useMemo, useState } from "react";
import type { ManifestEntry } from "../contracts";
import { formatBytes } from "../lib/format";
import { Icon } from "./Icon";

function fileType(file: ManifestEntry): string {
  if (file.kind === "directory") return "directory";
  const name = file.path.split(/[\\/]/).at(-1) ?? "";
  const dot = name.lastIndexOf(".");
  return dot > 0 && dot < name.length - 1
    ? name.slice(dot + 1).toUpperCase()
    : "none";
}

function typeLabel(type: string): string {
  return type === "directory"
    ? "Dossiers"
    : type === "none"
      ? "Sans extension"
      : type;
}

export function AssetFilesPanel({
  files,
  primaryFile,
  hidden = false,
}: {
  files: ManifestEntry[];
  primaryFile: string | null;
  hidden?: boolean;
}) {
  const [type, setType] = useState("all");
  const [minimum, setMinimum] = useState("");
  const [maximum, setMaximum] = useState("");
  const [unit, setUnit] = useState(1);
  const [limit, setLimit] = useState(100);
  const errorId = useId();
  const types = useMemo(
    () =>
      [...new Set(files.map(fileType))].sort((a, b) =>
        typeLabel(a).localeCompare(typeLabel(b), "fr"),
      ),
    [files],
  );
  const min = minimum === "" ? null : Number(minimum) * unit;
  const max = maximum === "" ? null : Number(maximum) * unit;
  const invalidNumber = [min, max].some(
    (value) => value !== null && (!Number.isFinite(value) || value < 0),
  );
  const error = invalidNumber
    ? "Saisissez des tailles positives ou nulles."
    : min !== null && max !== null && min > max
      ? "Le minimum ne peut pas dépasser le maximum."
      : "";
  const filtered = useMemo(
    () =>
      error
        ? []
        : files.filter(
            (file) =>
              (type === "all" || fileType(file) === type) &&
              ((min === null && max === null) ||
                (file.kind === "file" &&
                  (min === null || file.size >= min) &&
                  (max === null || file.size <= max))),
          ),
    [files, type, min, max, error],
  );
  const totalFiles = files.filter((file) => file.kind === "file").length;
  const shownFiles = filtered.filter((file) => file.kind === "file").length;
  const shownDirectories = filtered.length - shownFiles;
  const hasFilters = type !== "all" || minimum !== "" || maximum !== "";
  const reset = () => {
    setType("all");
    setMinimum("");
    setMaximum("");
    setUnit(1);
    setLimit(100);
  };

  return (
    <div role="tabpanel" className="file-panel" hidden={hidden}>
      <div
        className="file-filters"
        role="group"
        aria-label="Filtrer les fichiers"
      >
        <label>
          Type de fichier
          <select
            aria-label="Type de fichier"
            value={type}
            onChange={(event) => {
              setType(event.target.value);
              setLimit(100);
            }}
          >
            <option value="all">Tous les types</option>
            {types.map((value) => (
              <option value={value} key={value}>
                {typeLabel(value)}
              </option>
            ))}
            {type !== "all" && !types.includes(type) && (
              <option value={type}>{typeLabel(type)}</option>
            )}
          </select>
        </label>
        <label>
          Taille minimum
          <input
            type="number"
            min="0"
            step="any"
            aria-label="Taille minimum"
            value={minimum}
            aria-invalid={!!error}
            aria-describedby={error ? errorId : undefined}
            placeholder="Sans minimum"
            onChange={(event) => {
              setMinimum(event.target.value);
              setLimit(100);
            }}
          />
        </label>
        <label>
          Taille maximum
          <input
            type="number"
            min="0"
            step="any"
            aria-label="Taille maximum"
            value={maximum}
            aria-invalid={!!error}
            aria-describedby={error ? errorId : undefined}
            placeholder="Sans maximum"
            onChange={(event) => {
              setMaximum(event.target.value);
              setLimit(100);
            }}
          />
        </label>
        <label>
          Unité de taille
          <select
            aria-label="Unité de taille"
            value={unit}
            onChange={(event) => {
              setUnit(Number(event.target.value));
              setLimit(100);
            }}
          >
            <option value={1}>o</option>
            <option value={1024}>Ko</option>
            <option value={1024 ** 2}>Mo</option>
            <option value={1024 ** 3}>Go</option>
          </select>
        </label>
        <button
          type="button"
          className="text-button"
          disabled={!hasFilters && unit === 1}
          onClick={reset}
        >
          Réinitialiser les filtres
        </button>
      </div>
      {error ? (
        <p className="file-filter-error" role="alert" id={errorId}>
          {error}
        </p>
      ) : (
        <p className="file-filter-results" role="status">
          {shownFiles} {shownFiles === 1 ? "fichier" : "fichiers"} sur{" "}
          {totalFiles}
          {shownDirectories > 0 &&
            ` · ${shownDirectories} ${shownDirectories === 1 ? "dossier" : "dossiers"}`}
          {(min !== null || max !== null) &&
            " · Dossiers exclus du filtre de taille"}
        </p>
      )}
      {filtered.length ? (
        <>
          <div className="file-table-header">
            <span>NOM</span>
            <span>TYPE</span>
            <span>TAILLE</span>
          </div>
          {filtered.slice(0, limit).map((file) => (
            <div className="file-row" key={file.path}>
              <span>
                <Icon
                  name={file.kind === "directory" ? "folder" : "file"}
                  size={17}
                />
                <code title={file.path}>{file.path}</code>
                {file.path === primaryFile && (
                  <span className="primary-file-badge">Principal</span>
                )}
              </span>
              <span>
                {file.kind === "directory"
                  ? "Dossier"
                  : typeLabel(fileType(file))}
              </span>
              <span>
                {file.kind === "directory" ? "—" : formatBytes(file.size)}
              </span>
            </div>
          ))}
          {filtered.length > limit && (
            <button
              type="button"
              className="button secondary load-more"
              onClick={() => setLimit((value) => value + 100)}
            >
              Afficher les 100 suivants
            </button>
          )}
        </>
      ) : (
        !error && (
          <div className="small-empty">
            <Icon name={files.length ? "search" : "folder"} size={24} />
            <p>
              {files.length
                ? "Aucun fichier ne correspond aux filtres."
                : "Ce dossier est prêt à accueillir vos fichiers."}
            </p>
          </div>
        )
      )}
    </div>
  );
}
