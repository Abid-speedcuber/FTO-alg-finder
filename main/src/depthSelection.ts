export type DepthSelection =
  | { depths: number[] | null; error: null }
  | { depths: null; error: string };

/** Print-style selection: inclusive ranges and individual depths, sorted and unique. */
export function parseDepthSelection(input: string): DepthSelection {
  const value = input.trim();
  if (!value || value.toLowerCase() === "auto") return { depths: null, error: null };
  const depths = new Set<number>();
  for (const part of value.split(",")) {
    const match = part.trim().match(/^(\d+)\s*(?:-\s*(\d+))?$/);
    if (!match) return { depths: null, error: "Use auto, individual depths, or ranges, e.g. 12-15, 17, 20." };
    const start = Number(match[1]);
    const end = match[2] === undefined ? start : Number(match[2]);
    if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end) || start < 0 || end > 255) {
      return { depths: null, error: "Depths must be whole numbers from 0 to 255." };
    }
    if (start > end) return { depths: null, error: "Range start must be no greater than its end, e.g. 12-15." };
    for (let depth = start; depth <= end; depth++) depths.add(depth);
  }
  return { depths: [...depths].sort((a, b) => a - b), error: null };
}
