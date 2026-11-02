import type { Job } from "../contracts";
import { api } from "../api";
import { errorMessage } from "../lib/format";
import { Icon } from "./Icon";

const names: Record<Job["kind"], string> = {
  thumbnail: "Préparation de la vignette",
  import: "Import du dossier",
  create: "Création de l’asset",
  snapshot: "Création de la version",
  variant: "Création de la variante",
  changes: "Analyse des modifications",
  inspect: "Analyse 3D",
  preview: "Préparation de l’aperçu",
  export: "Export GLB",
};

export function Jobs({
  jobs,
  dismiss,
  onError,
}: {
  jobs: Job[];
  dismiss: (id: string) => void;
  onError: (message: string) => void;
}) {
  const visible = jobs
    .filter(
      (job) =>
        job.kind !== "thumbnail" &&
        (job.status === "running" ||
          job.status === "queued" ||
          job.status === "failed" ||
          !["changes", "inspect", "preview"].includes(job.kind)),
    )
    .slice(0, 3);
  if (!visible.length) return null;
  return (
    <aside
      className="job-stack"
      aria-label="Opérations en arrière-plan"
      aria-live="polite"
    >
      {visible.map((job) => {
        const active = job.status === "running" || job.status === "queued";
        const ratio = job.total
          ? Math.min(100, Math.round((job.completed / job.total) * 100))
          : null;
        return (
          <div className={`job-card ${job.status}`} key={job.id}>
            <div className={`job-indicator ${active ? "spinning" : ""}`}>
              <Icon
                name={
                  active
                    ? "refresh"
                    : job.status === "completed"
                      ? "check"
                      : job.status === "failed"
                        ? "warning"
                        : "close"
                }
                size={17}
              />
            </div>
            <div className="job-body">
              <strong>{names[job.kind]}</strong>
              <p>
                {job.error?.message ??
                  (job.status === "completed"
                    ? "Terminé"
                    : job.status === "cancelled"
                      ? "Annulé"
                      : job.phase || "En attente…")}
              </p>
              {active && (
                <div
                  className={`progress-track ${ratio == null ? "indeterminate" : ""}`}
                >
                  <span
                    style={ratio == null ? undefined : { width: `${ratio}%` }}
                  />
                </div>
              )}
            </div>
            <button
              className="icon-button"
              aria-label={
                active ? "Annuler l’opération" : "Masquer l’opération"
              }
              onClick={() => {
                if (active)
                  void api
                    .cancelJob(job.id)
                    .catch((error) => onError(errorMessage(error)));
                else dismiss(job.id);
              }}
            >
              <Icon name="close" size={15} />
            </button>
          </div>
        );
      })}
    </aside>
  );
}
