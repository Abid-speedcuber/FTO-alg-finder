/** Execution holds use {original face at U, original face at F}.
 * See https://sites.google.com/view/cfl-fto/notation .
 */
export const FACES = ["U", "F", "BR", "BL", "D", "B", "R", "L"] as const;
export type Face = typeof FACES[number];
export type ExecutionHold = {
  input: string;
  notation: "CIF" | "EIF";
  // Original face -> face name when executing in this hold.
  faces: Record<Face, Face>;
};

type Vector = readonly [number, number, number];
const OPPOSITE: Record<Face, Face> = { U: "D", D: "U", F: "B", B: "F", R: "BL", BL: "R", L: "BR", BR: "L" };
const SIDES: Face[] = ["F", "L", "BL", "B", "BR", "R"];
// The same outward face normals used by poly3dlib's octahedron, in CIF.
const NORMALS: Record<Face, Vector> = {
  U: [0, 1, 0], D: [0, -1, 0],
  R: [Math.sqrt(6) / 3, 1 / 3, Math.sqrt(2) / 3],
  BL: [-Math.sqrt(6) / 3, -1 / 3, -Math.sqrt(2) / 3],
  L: [-Math.sqrt(6) / 3, 1 / 3, Math.sqrt(2) / 3],
  BR: [Math.sqrt(6) / 3, -1 / 3, -Math.sqrt(2) / 3],
  F: [0, -1 / 3, Math.sqrt(8) / 3], B: [0, 1 / 3, -Math.sqrt(8) / 3],
};
const dot = (a: Vector, b: Vector) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: Vector, b: Vector): Vector => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const project = (v: Vector, axis: Vector): Vector => {
  const along = dot(v, axis);
  return [v[0] - along * axis[0], v[1] - along * axis[1], v[2] - along * axis[2]];
};

export function parseExecutionHold(input: string): ExecutionHold | null {
  const normalized = input.trim().toUpperCase().replace(/\s/g, "");
  const parts = (normalized || "U,F").split(",");
  if (parts.length !== 2 || !parts.every((face) => FACES.includes(face as Face))) return null;
  const [up, front] = parts as [Face, Face];
  if (front === up || front === OPPOSITE[up]) return null;
  const axis = NORMALS[up];
  const forward = project(NORMALS[front], axis);
  const angle = (face: Face) => {
    if (face === front) return 0;
    const v = project(NORMALS[face], axis);
    return (Math.atan2(-dot(axis, cross(forward, v)), dot(forward, v)) + 2 * Math.PI) % (2 * Math.PI);
  };
  const ring = FACES.filter((face) => face !== up && face !== OPPOSITE[up])
    .sort((a, b) => angle(a) - angle(b));
  const faces = { [up]: "U", [OPPOSITE[up]]: "D" } as Record<Face, Face>;
  ring.forEach((original, i) => { faces[original] = SIDES[i]; });
  return {
    input: `${up},${front}`,
    notation: dot(axis, NORMALS[front]) < 0 ? "CIF" : "EIF",
    faces,
  };
}

export const DEFAULT_HOLD = parseExecutionHold("U,F")!;

// Keep legacy spellings readable while emitting the standard notation.
export function normalizeNotation(algorithm: string): string {
  return algorithm.replace(/\b(BR|BL|[UDFRLB])w(['’2i]?)(?![A-Za-z0-9])|\b([MESN])(['’2i]?)(?![A-Za-z0-9])/g,
    (_token, face: string | undefined, wideSuffix: string, slice: string, suffix: string) => {
      if (face) return face.toLowerCase() + (wideSuffix === "i" ? "'" : wideSuffix);
      const inverse = suffix.length > 0;
      const base = slice === "M" ? "Rs" : slice === "E" ? "Us" : slice === "N" ? "Ls" : "Fs";
      return base + ((slice === "S" || slice === "N") === inverse ? "'" : "");
    });
}

export function normalizeMoveSelection(moves: string[]): string[] {
  return moves.map(normalizeNotation).filter(move => !/^\([RF]'? U'? [RF]'?\)$/.test(move));
}

export function rotateAlgorithm(algorithm: string, hold: ExecutionHold): string {
  return normalizeNotation(algorithm).replace(/\b(BR|BL|[UDFRLB])s(['’2]?)(?![A-Za-z0-9])|\b(br|bl|[udfrlb]|BR|BL|[UDFRLB])(['’2]?)(?![A-Za-z0-9])/g,
    (_token, slice: Face | undefined, sliceSuffix: string, base: string, suffix: string) => {
      if (slice) {
        const mapped = hold.faces[slice];
        if (["D", "B", "BR", "BL"].includes(mapped)) {
          return `${OPPOSITE[mapped]}s${sliceSuffix ? "" : "'"}`;
        }
        return `${mapped}s${sliceSuffix}`;
      }
      const wide = base === base.toLowerCase();
      const mapped = hold.faces[base.toUpperCase() as Face];
      return `${wide ? mapped.toLowerCase() : mapped}${suffix}`;
    });
}

/** Resolve absolute input holds before inversion or execution. Markers persist
 * until the next marker, and each algorithm starts in the default hold. */
export function resolveInputAlgorithm(algorithm: string): string {
  let hold = DEFAULT_HOLD;
  return (algorithm.match(/\{[^}]*\}|[^\s(){}]+|[{}]/g) ?? []).flatMap(token => {
    if (token.startsWith("{")) {
      const parsed = token.endsWith("}") && token.slice(1, -1).trim() ? parseExecutionHold(token.slice(1, -1)) : null;
      if (!parsed) throw new Error(`Invalid input orientation: ${token}`);
      hold = parsed;
      return [];
    }
    if (token === "}") throw new Error("Unexpected } in algorithm");
    const faces = Object.fromEntries(FACES.map(face => [hold.faces[face], face])) as Record<Face, Face>;
    return [rotateAlgorithm(token.replace(/[’i]$/, "'"), { ...hold, faces })];
  }).join(" ");
}

export function invertInputAlgorithm(algorithm: string): string {
  return resolveInputAlgorithm(algorithm).split(/\s+/).filter(Boolean).reverse().map(move =>
    /['’2i]$/.test(move) ? move.slice(0, -1) : `${move}'`,
  ).join(" ");
}
