export function formatBytes(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return "—";
  if (value <= 0) return "0 o";
  const exponent = Math.min(Math.floor(Math.log(value) / Math.log(1024)), 4);
  const amount = new Intl.NumberFormat("fr-FR", {
    maximumFractionDigits: exponent === 0 ? 0 : 1,
  }).format(value / 1024 ** exponent);
  return `${amount} ${["o", "Ko", "Mo", "Go", "To"][exponent]}`;
}

export function formatCount(value: number | null | undefined): string {
  return value == null ? "—" : new Intl.NumberFormat("fr-FR").format(value);
}

export function formatDate(value: string | number | null | undefined): string {
  if (value == null) return "—";
  const date = new Date(
    typeof value === "number" && value < 100_000_000_000 ? value * 1000 : value,
  );
  return Number.isNaN(date.getTime())
    ? String(value)
    : new Intl.DateTimeFormat("fr-FR", {
        day: "numeric",
        month: "short",
        year: "numeric",
      }).format(date);
}

export function errorMessage(value: unknown): string {
  if (value instanceof Error) return value.message;
  if (typeof value === "string") return value;
  if (
    value &&
    typeof value === "object" &&
    "message" in value &&
    typeof value.message === "string"
  )
    return value.message;
  return "Une erreur inattendue est survenue. Réessayez après avoir vérifié le dossier.";
}

export function extension(path: string | null | undefined): string {
  return path?.split(".").at(-1)?.toUpperCase() ?? "DOSSIER";
}
