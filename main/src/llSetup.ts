// Last-layer setup: facelet model, option application, and the click planner.
//
// Diagram frame (the flattened top layer used by the thumbnails). Yellow points
// down. The strips are the faces adjacent to yellow: top = green, left = orange,
// right = gray. Slot names are positions in this frame, never colors.
//
//        TL ---- top strip ---- TR
//          \                  /
//     left strip   yellow   right strip
//            \              /
//                  BOT
//
// EP slots: T1/T2 = top strip (next to TL / TR), L1/L2 = left strip
// (next to TL / BOT), R1/R2 = right strip (next to TR / BOT).
// CP slots: TL, TR, BOT.

export const FACELET_COUNT = 72;

export const SOLVED_FACELETS: number[] = Array.from({ length: FACELET_COUNT }, (_, i) =>
  Math.floor(i / 9),
);

export const CP_SLOTS = ["TL", "TR", "BOT"] as const;
export type CpSlot = (typeof CP_SLOTS)[number];

export const EP_SLOTS = ["T1", "T2", "R1", "R2", "L1", "L2"] as const;
export type EpSlot = (typeof EP_SLOTS)[number];

// Corners in the viewer's piece order; index i of one slot maps to index i of
// the next under U. TL touches the top and left strips, TR the top and right,
// BOT the left and right.
export const CP_FACELETS: Record<CpSlot, number[]> = {
  TL: [8, 67, 35, 49],
  TR: [4, 53, 22, 62],
  BOT: [0, 54, 9, 63],
};

export const EP_FACELET: Record<EpSlot, number> = {
  T1: 50,
  T2: 52,
  R1: 61,
  R2: 56,
  L1: 68,
  L2: 65,
};

export const LL_EDGE_FACELETS = [3, 64, 6, 51, 1, 57];
export const LL_YELLOW_CENTERS = [2, 7, 5];
export const LL_FRAME_FACELETS = [...LL_EDGE_FACELETS, ...LL_YELLOW_CENTERS];

// U as content flow: for a cycle [a, b, c], the color on a ends up on b.
const U_CYCLES: number[][] = [
  [0, 8, 4],
  [54, 67, 53],
  [9, 35, 22],
  [63, 49, 62],
  [3, 6, 1],
  [64, 51, 57],
  [2, 7, 5],
  [65, 50, 61],
  [68, 52, 56],
];

export const LL_FACELETS: number[] = U_CYCLES.flat();
const LL_FACELET_SET = new Set(LL_FACELETS);
export const F2L_FACELETS: number[] = Array.from({ length: FACELET_COUNT }, (_, i) => i).filter(
  (i) => !LL_FACELET_SET.has(i),
);

export const EP_FACELET_SET = new Set(EP_SLOTS.map((slot) => EP_FACELET[slot]));

export type LlGroup = "EP" | "CP";
export type LlSelection = { EP: string | null; CP: string | null };
export const EMPTY_LL_SELECTION: LlSelection = { EP: null, CP: null };

export type EpOption = {
  id: string;
  label: string;
  perm: Record<EpSlot, EpSlot>;
};

export type CpOption = {
  id: string;
  label: string;
  row: 1 | 2;
  perm: Record<CpSlot, CpSlot>;
  twists: CpSlot[];
};

export type LlPlan = {
  next: number[];
  selection: LlSelection;
  resets: string[];
};

function permFromCycles<T extends string>(slots: readonly T[], cycles: T[][]): Record<T, T> {
  const perm = {} as Record<T, T>;
  for (const slot of slots) {
    perm[slot] = slot;
  }
  for (const cycle of cycles) {
    for (let i = 0; i < cycle.length; i++) {
      perm[cycle[(i + 1) % cycle.length]] = cycle[i];
    }
  }
  return perm;
}

// Cycles are written as content flow: ["T1", "R1"] moves T1's piece to R1.
export function epPerm(cycles: EpSlot[][]): Record<EpSlot, EpSlot> {
  return permFromCycles(EP_SLOTS, cycles);
}

export function cpPerm(cycles: CpSlot[][]): Record<CpSlot, CpSlot> {
  return permFromCycles(CP_SLOTS, cycles);
}

export function sameFacelets(a: number[] | null, b: number[] | null): boolean {
  if (!a || !b || a.length !== b.length) {
    return false;
  }
  return a.every((value, index) => value === b[index]);
}

export function applyU(facelets: number[]): number[] {
  const out = facelets.slice();
  for (const cycle of U_CYCLES) {
    for (let i = 0; i < cycle.length; i++) {
      out[cycle[(i + 1) % cycle.length]] = facelets[cycle[i]];
    }
  }
  return out;
}

export function applyUPrime(facelets: number[]): number[] {
  return applyU(applyU(facelets));
}

export function rotatedSolved(offset: number): number[] {
  let out = SOLVED_FACELETS.slice();
  for (let i = 0; i < offset; i++) {
    out = applyU(out);
  }
  return out;
}

// Number of U turns between the solved top edges and their current position.
export function edgeOffset(facelets: number[]): number | null {
  for (let k = 0; k < 3; k++) {
    const target = rotatedSolved(k);
    if (LL_EDGE_FACELETS.every((index) => facelets[index] === target[index])) {
      return k;
    }
  }
  return null;
}

export function isF2lSolved(facelets: number[]): boolean {
  return F2L_FACELETS.every((index) => facelets[index] === SOLVED_FACELETS[index]);
}

function copyFrom(facelets: number[], source: number[], indices: number[]): number[] {
  const out = facelets.slice();
  for (const index of indices) {
    out[index] = source[index];
  }
  return out;
}

function cornerAt(facelets: number[], slot: CpSlot): { piece: number; twist: number } | null {
  const observed = CP_FACELETS[slot].map((index) => facelets[index]);
  for (let piece = 0; piece < CP_SLOTS.length; piece++) {
    const home = CP_FACELETS[CP_SLOTS[piece]];
    for (const twist of [0, 2]) {
      const matches = observed.every(
        (color, i) => color === SOLVED_FACELETS[home[(i + twist) % 4]],
      );
      if (matches) {
        return { piece, twist: twist === 0 ? 0 : 1 };
      }
    }
  }
  return null;
}

function permutationParity(values: number[]): number {
  let parity = 0;
  for (let i = 0; i < values.length; i++) {
    for (let j = i + 1; j < values.length; j++) {
      if (values[i] > values[j]) {
        parity ^= 1;
      }
    }
  }
  return parity;
}

// Legal without touching F2L: the three top corners, an even permutation, and
// an even number of twists. Rejects a 2-swap and a 1- or 3-twist.
export function isCpIntact(facelets: number[]): boolean {
  const pieces: number[] = [];
  let twists = 0;
  for (const slot of CP_SLOTS) {
    const corner = cornerAt(facelets, slot);
    if (!corner) {
      return false;
    }
    pieces.push(corner.piece);
    twists += corner.twist;
  }
  if (new Set(pieces).size !== CP_SLOTS.length) {
    return false;
  }
  return twists % 2 === 0 && permutationParity(pieces) === 0;
}

// Centers carry no permutation parity, so any arrangement of the six is legal.
export function isEpIntact(facelets: number[]): boolean {
  const counts = new Map<number, number>();
  for (const slot of EP_SLOTS) {
    const color = facelets[EP_FACELET[slot]];
    counts.set(color, (counts.get(color) ?? 0) + 1);
  }
  return [5, 6, 7].every((color) => counts.get(color) === 2) && counts.size === 3;
}

export function isGroupIntact(facelets: number[], group: LlGroup): boolean {
  return group === "EP" ? isEpIntact(facelets) : isCpIntact(facelets);
}

function groupFacelets(group: LlGroup): number[] {
  return group === "EP"
    ? EP_SLOTS.map((slot) => EP_FACELET[slot])
    : CP_SLOTS.flatMap((slot) => CP_FACELETS[slot]);
}

export function solveGroup(facelets: number[], group: LlGroup, offset: number): number[] {
  return copyFrom(facelets, rotatedSolved(offset), groupFacelets(group));
}

export function applyEpOption(facelets: number[], option: EpOption): number[] {
  const out = facelets.slice();
  for (const dest of EP_SLOTS) {
    out[EP_FACELET[dest]] = facelets[EP_FACELET[option.perm[dest]]];
  }
  return out;
}

export function applyCpOption(facelets: number[], option: CpOption): number[] {
  const out = facelets.slice();
  for (const dest of CP_SLOTS) {
    const source = CP_FACELETS[option.perm[dest]];
    const target = CP_FACELETS[dest];
    const twist = option.twists.includes(dest) ? 2 : 0;
    for (let i = 0; i < target.length; i++) {
      out[target[i]] = facelets[source[(i + twist) % target.length]];
    }
  }
  return out;
}

export function applyLlOption(
  facelets: number[],
  group: LlGroup,
  option: EpOption | CpOption,
): number[] {
  return group === "EP"
    ? applyEpOption(facelets, option as EpOption)
    : applyCpOption(facelets, option as CpOption);
}

// Everything a click does, computed without touching the cube. `resets` is
// empty when the click is safe; otherwise it names what the modal will destroy.
export function planLlOption(
  facelets: number[],
  selection: LlSelection,
  group: LlGroup,
  option: EpOption | CpOption,
): LlPlan {
  let work = facelets.slice();
  const resets: string[] = [];
  const nextSelection: LlSelection = { ...selection };

  if (!isF2lSolved(work)) {
    resets.push("F2L");
    work = copyFrom(work, SOLVED_FACELETS, F2L_FACELETS);
  }

  let offset = edgeOffset(work);
  const centersOk = LL_YELLOW_CENTERS.every((index) => work[index] === 0);
  if (offset === null || !centersOk) {
    resets.push("the last layer edges");
    offset = offset ?? 0;
    work = copyFrom(work, rotatedSolved(offset), LL_FRAME_FACELETS);
  }

  const other: LlGroup = group === "EP" ? "CP" : "EP";
  if (!isGroupIntact(work, other)) {
    resets.push(other === "CP" ? "the last layer corners" : "the last layer centers");
    work = solveGroup(work, other, offset);
    nextSelection[other] = null;
  }

  work = solveGroup(work, group, offset);
  if (selection[group] === option.id) {
    nextSelection[group] = null;
  } else {
    work = applyLlOption(work, group, option);
    nextSelection[group] = option.id;
  }

  return { next: work, selection: nextSelection, resets };
}

export function formatResets(resets: string[]): string {
  if (resets.length <= 1) {
    return resets[0] ?? "";
  }
  return `${resets.slice(0, -1).join(", ")} and ${resets[resets.length - 1]}`;
}
