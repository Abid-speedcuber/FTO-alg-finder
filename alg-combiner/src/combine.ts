export type AlgSet = Record<string, string>;

export type Candidate = {
  names: string[];
  tokens: string[];
  text: string;
  moves: number;
};

export type TransitionSource = (token: string) => number[] | null;

const NUM_FACELETS = 72;
const SLOT_OPTIONS = ["", "U", "U'"];

export function parseIntermediateAlgs(text: string): AlgSet {
  const algs: AlgSet = {};
  let auto = 0;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith("#") || line.startsWith("//")) {
      continue;
    }
    const colon = line.indexOf(":");
    let name: string;
    let moves: string;
    if (colon === -1) {
      auto += 1;
      name = `alg_${auto}`;
      moves = line;
    } else {
      auto += 1;
      name = line.slice(0, colon).trim() || `alg_${auto}`;
      moves = line.slice(colon + 1).trim();
    }
    if (tokenizeAlg(moves).length === 0) {
      continue;
    }
    algs[name] = moves;
  }
  return algs;
}

export function serializeIntermediateAlgs(algs: AlgSet): string {
  return Object.entries(algs)
    .map(([name, moves]) => `${name}: ${moves}`)
    .join("\n");
}

export function tokenizeAlg(moves: string): string[] {
  return moves.replace(/[()]/g, " ").trim().split(/\s+/).filter(Boolean);
}

export function invertMove(move: string): string {
  if (move.endsWith("'") || move.endsWith("i")) {
    return move.slice(0, -1);
  }
  const half = /^(.*?)2$/.exec(move);
  if (half) {
    // FTO face turns have order 3, so X2 followed by X is the identity.
    return half[1];
  }
  return `${move}'`;
}

export function invertTokens(tokens: string[]): string[] {
  const out: string[] = [];
  for (let i = tokens.length - 1; i >= 0; i--) {
    out.push(invertMove(tokens[i]));
  }
  return out;
}

/**
 * Decompose a move into (face, power) where power is 1 (forward) or 2 (double =
 * inverse on an order-3 FTO turn). Handles X, X', X2, and wide moves like Uw, Uw2.
 */
export function parseMoveToken(token: string): { base: string; power: 1 | 2 } | null {
  if (!token) {
    return null;
  }
  if (token.endsWith("'")) {
    return { base: token.slice(0, -1), power: 2 };
  }
  if (token.endsWith("2")) {
    const base = token.slice(0, -1);
    if (base.endsWith("'")) {
      return { base: base.slice(0, -1), power: 1 };
    }
    return { base, power: 2 };
  }
  return { base: token, power: 1 };
}

/**
 * Cancels adjacent same-face moves, recursively: X X' and X' X vanish; X X becomes
 * X'; X' X' becomes X (X2 X2 becomes X). Lifting a cancelled pair re-unites the
 * neighbours, which are then combined too, so cancellations cascade.
 */
export function cancelTokens(tokens: string[]): string[] {
  const out: { base: string; power: 1 | 2 }[] = [];
  for (const token of tokens) {
    const move = parseMoveToken(token);
    if (!move) {
      out.push({ base: token, power: 1 });
      continue;
    }
    const last = out[out.length - 1];
    if (last && last.base === move.base) {
      const power = ((last.power + move.power) % 3) as 1 | 2 | 0;
      if (power === 0) {
        out.pop();
      } else {
        out[out.length - 1] = { base: last.base, power };
      }
    } else {
      out.push({ base: move.base, power: move.power });
    }
  }
  return out.map((move) => (move.power === 2 ? `${move.base}'` : move.base));
}

export const solvedFacelets: readonly number[] = (() => {
  const out = new Array<number>(NUM_FACELETS);
  for (let i = 0; i < NUM_FACELETS; i++) {
    out[i] = Math.floor(i / 9);
  }
  return out;
})();

export function isSolved(facelets: number[]): boolean {
  for (let i = 0; i < NUM_FACELETS; i++) {
    if (facelets[i] !== Math.floor(i / 9)) {
      return false;
    }
  }
  return true;
}

export function countCombinations(algCount: number, useTriples: boolean) {
  const singles = algCount * 9;
  const pairs = algCount * (algCount - 1) * 27;
  const triples = useTriples ? algCount * (algCount - 1) * (algCount - 2) * 81 : 0;
  return { singles, pairs, triples, total: singles + pairs + triples };
}

export function* generateCombos(
  algs: AlgSet,
  useTriples: boolean,
): Generator<Candidate> {
  const names = Object.keys(algs);
  const tokenBy: Record<string, string[]> = {};
  for (const name of names) {
    tokenBy[name] = tokenizeAlg(algs[name]);
  }
  const seen = new Set<string>();

  for (let i = 0; i < names.length; i++) {
    yield* expandSlots([names[i]], tokenBy, seen);
  }

  for (let i = 0; i < names.length; i++) {
    for (let j = 0; j < names.length; j++) {
      if (i === j) {
        continue;
      }
      yield* expandSlots([names[i], names[j]], tokenBy, seen);
    }
  }

  if (useTriples) {
    for (let i = 0; i < names.length; i++) {
      for (let j = 0; j < names.length; j++) {
        if (j === i) {
          continue;
        }
        for (let k = 0; k < names.length; k++) {
          if (k === i || k === j) {
            continue;
          }
          yield* expandSlots([names[i], names[j], names[k]], tokenBy, seen);
        }
      }
    }
  }
}

function* expandSlots(
  group: string[],
  tokenBy: Record<string, string[]>,
  seen: Set<string>,
): Generator<Candidate> {
  const slotCount = group.length + 1;
  const total = Math.pow(3, slotCount);
  const partTokens = group.map((name) => tokenBy[name]);

  for (let code = 0; code < total; code++) {
    const slots: string[] = [];
    let c = code;
    for (let s = 0; s < slotCount; s++) {
      slots[s] = SLOT_OPTIONS[c % 3];
      c = (c / 3) | 0;
    }
    const tokens: string[] = [];
    for (let g = 0; g < group.length; g++) {
      if (slots[g]) {
        tokens.push(slots[g]);
      }
      tokens.push(...partTokens[g]);
    }
    if (slots[slotCount - 1]) {
      tokens.push(slots[slotCount - 1]);
    }
    const text = tokens.join(" ");
    if (seen.has(text)) {
      continue;
    }
    seen.add(text);
    yield {
      names: group.slice(),
      tokens,
      text,
      moves: tokens.length,
    };
  }
}

export class MoveEngine {
  private cache = new Map<string, number[] | null>();
  private bufA = new Array<number>(NUM_FACELETS);
  private bufB = new Array<number>(NUM_FACELETS);

  constructor(private readonly source: TransitionSource) {}

  transition(token: string): number[] | null {
    if (this.cache.has(token)) {
      return this.cache.get(token) as number[] | null;
    }
    const t = this.source(token);
    this.cache.set(token, t);
    return t;
  }

  /** Returns true when applying tokens to base reaches the solved state. */
  solves(base: number[], tokens: string[]): boolean {
    if (tokens.length === 0) {
      return isSolved(base);
    }
    let a = this.bufA;
    let b = this.bufB;
    for (let j = 0; j < NUM_FACELETS; j++) {
      a[j] = base[j];
    }
    for (const token of tokens) {
      const t = this.transition(token);
      if (!t) {
        return false;
      }
      for (let j = 0; j < NUM_FACELETS; j++) {
        b[j] = a[t[j]];
      }
      const tmp = a;
      a = b;
      b = tmp;
    }
    return isSolved(a);
  }
}