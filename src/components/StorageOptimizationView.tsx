import { useEffect, useRef, useState } from "react";
import type { Job, StorageDiagnostic } from "../contracts";
import type { RunJob } from "../lib/useJobs";
import { errorMessage, formatBytes, formatCount } from "../lib/format";
import { HistoryStorageModal } from "./HistoryStorageModal";
import { Icon } from "./Icon";

export function StorageOptimizationView({
  libraryRoot,
  jobs,
  runJob,
}: {
  libraryRoot: string;
  jobs: Job[];
  runJob: RunJob;
}) {
  const [report, setReport] = useState<StorageDiagnostic | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [cancelled, setCancelled] = useState(false);
  const [stale, setStale] = useState(false);
  const [action, setAction] = useState<"recompress" | "purge" | null>(null);
  const controller = useRef<AbortController | null>(null);
  useEffect(() => () => controller.current?.abort(), []);

  const diagnose = async () => {
    const next = new AbortController();
    controller.current?.abort();
    controller.current = next;
    setBusy(true);
    setError("");
    setCancelled(false);
    try {
      const result = await runJob<StorageDiagnostic>(
        { kind: "storageDiagnostic" },
        { signal: next.signal },
      );
      if (!next.signal.aborted) {
        setReport(result);
        setStale(false);
      }
    } catch (reason) {
      if (!next.signal.aborted) setError(errorMessage(reason));
    } finally {
      if (!next.signal.aborted) setBusy(false);
    }
  };
  const active = jobs.find(
    (job) =>
      job.kind === "storageDiagnostic" &&
      (job.status === "running" || job.status === "queued"),
  );
  const detail = (key: string) =>
    report?.details.find((item) => item.key === key);
  const purge = detail("purgeCandidates");
  const orphan = detail("unreferencedObjects");
  const recoveryBytes =
    (detail("publicationRecovery")?.bytes ?? 0) +
    (detail("restorationRecovery")?.bytes ?? 0);

  return (
    <section className="library-content storage-optimization">
      <div className="page-heading">
        <div>
          <span className="eyebrow">BIBLIOTHÈQUE LOCALE</span>
          <h1>Optimisation du stockage</h1>
          <p>
            Mesurez l’espace occupé et gérez les objets de toute la
            bibliothèque.
          </p>
        </div>
      </div>
      <p className="storage-library-path">
        <code>{libraryRoot}</code>
      </p>
      <div className="storage-tools">
        <article className="storage-tool">
          <Icon name="database" />
          <h2>Diagnostic du stockage</h2>
          <p>
            Inventaire en lecture seule des fichiers et de leurs références.
            Aucun fichier n’est supprimé ou modifié.
          </p>
          <div className="storage-tool-actions">
            <button
              className="button primary"
              disabled={busy || !!action}
              onClick={() => void diagnose()}
            >
              {busy
                ? "Analyse en cours…"
                : report
                  ? "Actualiser le diagnostic"
                  : "Lancer le diagnostic"}
            </button>
            {busy && (
              <button
                className="button secondary"
                onClick={() => {
                  controller.current?.abort();
                  setBusy(false);
                  setCancelled(true);
                }}
              >
                Annuler le diagnostic
              </button>
            )}
          </div>
        </article>
        <article className="storage-tool">
          <Icon name="database" />
          <h2>Compresser avec Zstd</h2>
          <p>
            Compression sans perte des objets lorsque leur taille peut être
            réduite.
          </p>
          <p className="field-help">Portée : toute la bibliothèque</p>
          <button
            className="button secondary"
            disabled={busy || !!action}
            onClick={() => setAction("recompress")}
          >
            Compresser avec Zstd
          </button>
        </article>
        <article className="storage-tool">
          <Icon name="database" />
          <h2>Libérer les objets inutilisés</h2>
          <p>
            Supprime les objets libérés par l’historique et qui ne sont plus
            référencés. Les orphelins à examiner restent conservés.
          </p>
          <p className="field-help">Portée : toute la bibliothèque</p>
          <button
            className="button secondary"
            disabled={busy || !!action || report?.purgeAvailable === false}
            onClick={() => setAction("purge")}
          >
            Libérer les objets inutilisés
          </button>
        </article>
      </div>
      {busy && (
        <div className="storage-scan-progress" role="status">
          <p>{active?.phase || "Préparation du diagnostic…"}</p>
          {active && (
            <p>
              {formatCount(active.completed)} fichiers ou éléments parcourus
            </p>
          )}
          <progress
            aria-label="Progression du diagnostic"
            value={active?.total ? active.completed : undefined}
            max={active?.total ?? undefined}
          />
        </div>
      )}
      {cancelled && (
        <p role="status">
          Diagnostic annulé. Le dernier rapport disponible est conservé.
        </p>
      )}
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
      {report && (
        <div className="storage-diagnostic-report">
          <div className="storage-report-heading">
            <h2>Diagnostic du stockage</h2>
            <p>
              Analyse terminée le{" "}
              {new Date(report.completedAt).toLocaleString("fr-FR")}
            </p>
          </div>
          {stale && (
            <p className="notice-banner" role="status">
              Stockage modifié : actualisez le diagnostic pour obtenir les
              nouveaux volumes.
            </p>
          )}
          {!report.complete && (
            <p className="notice-banner" role="status">
              Rapport partiel : certains chemins n’ont pas pu être analysés. Les
              volumes mesurés peuvent être incomplets.
            </p>
          )}
          <div className="storage-summary">
            <div>
              <span>Total mesuré</span>
              <strong>{formatBytes(report.total.bytes)}</strong>
              <span>{formatCount(report.total.fileCount)} fichiers</span>
            </div>
            <div>
              <span>Libérable par la purge actuelle</span>
              <strong>
                {formatBytes(report.purgeAvailable ? (purge?.bytes ?? 0) : 0)}
              </strong>
              <span>
                {report.purgeAvailable
                  ? "Candidats connus sans référence"
                  : "Purge bloquée"}
              </span>
            </div>
            <div>
              <span>Sans référence, à examiner</span>
              <strong>{formatBytes(orphan?.bytes ?? 0)}</strong>
              <span>Non déclaré supprimable</span>
            </div>
            <div>
              <span>Récupération nécessaire</span>
              <strong>{formatBytes(recoveryBytes)}</strong>
              <span>Dossiers avec journal détecté</span>
            </div>
          </div>
          {report.purgeBlockedReason && (
            <p className="inline-error">
              Purge indisponible : {report.purgeBlockedReason}
            </p>
          )}
          <p className="field-help">
            Les tailles mesurent les fichiers, pas leur allocation sur le
            disque. Les applications externes peuvent modifier les workspaces
            pendant l’analyse. Le contenu des fichiers n’a pas été vérifié.
          </p>
          <div className="storage-table-wrap">
            <table className="storage-table">
              <caption>Répartition du stockage de la bibliothèque</caption>
              <thead>
                <tr>
                  <th>Catégorie</th>
                  <th>Fichiers</th>
                  <th>Taille mesurée</th>
                </tr>
              </thead>
              <tbody>
                {report.categories.map((category) => (
                  <tr key={category.key}>
                    <th scope="row">{category.label}</th>
                    <td>{formatCount(category.fileCount)}</td>
                    <td>{formatBytes(category.bytes)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p>
            Contenu logique unique des objets référencés, avant compression :{" "}
            <strong>{formatBytes(report.logicalObjectBytes)}</strong>.
          </p>
          <p className="field-help">
            Les détails suivants décrivent les catégories ci-dessus et ne
            s’ajoutent pas au total. Des représentations brut/Zstd simultanées
            sont signalées sans contrôle de leur contenu.
          </p>
          <div className="storage-diagnostic-details">
            {report.details.map((item) => (
              <details key={item.key}>
                <summary>
                  <span>{item.label}</span>
                  <span>
                    {formatCount(item.count)} · {formatBytes(item.bytes)}
                  </span>
                </summary>
                {item.examples.length > 0 ? (
                  <>
                    <p className="field-help">
                      {item.examples.length} exemple
                      {item.examples.length > 1 ? "s" : ""} affiché
                      {item.examples.length > 1 ? "s" : ""}, au maximum 20.
                    </p>
                    <ul>
                      {item.examples.map((example, index) => (
                        <li key={`${example.path}-${index}`}>
                          <code>{example.path}</code>
                          <span>{formatBytes(example.size)}</span>
                        </li>
                      ))}
                    </ul>
                  </>
                ) : (
                  <p className="field-help">Aucun exemple à afficher.</p>
                )}
              </details>
            ))}
          </div>
          {report.warnings.length > 0 && (
            <div className="storage-warnings">
              <h3>Points à examiner</h3>
              <ul>
                {report.warnings.map((warning, index) => (
                  <li key={index}>{warning}</li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
      {action && (
        <HistoryStorageModal
          action={action}
          runJob={runJob}
          onSaved={() => setStale(true)}
          onClose={() => setAction(null)}
        />
      )}
    </section>
  );
}
