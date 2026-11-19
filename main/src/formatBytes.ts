const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];

export function formatBytes(bytes: number | undefined): string {
  if (bytes === undefined || !Number.isFinite(bytes) || bytes < 0) return "Unknown";
  let value = bytes, unit = 0;
  while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit++; }
  if (unit && Math.round(value * 10) / 10 >= 1024 && unit < units.length - 1) {
    value /= 1024; unit++;
  }
  return `${unit ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}
