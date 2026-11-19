export function progressPercent(text: string): number {
  const match = text.match(/\((\d+(?:\.\d+)?)%\)/);
  return match ? Math.max(0, Math.min(100, Number(match[1]))) : 0;
}

// Counters change on every update; the phase and table identify the same bar.
export function progressKey(text: string): string {
  return text.split(/\s+reached\s+|\s+\d+\/\d+/)[0];
}
