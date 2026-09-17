import type { AlgSet } from "./combine";

// The "big object" of intermediate algs. Write down small building-block
// algorithms here (or live in the UI textarea). Each entry maps a name to a
// move sequence in FTO notation.
export const DEFAULT_INTERMEDIATE_ALGS: AlgSet = {
  "alg-1": "F U BR U' BR' F'",
  "alg-2": "F BR U BR' U' F'",

  "U+": "F U F' U F U F'",
  "U-": "F U' F' U' F U' F'",

  "alg-5": "F BR' F' L F BR F' L'",
  "alg-6": "L F BR' F' L' F BR F'",
  "alg-7": "L' BL' BR BL L BL' BR' BL",
  "alg-8": "BL' BR BL L' BL' BR' BL L",

  "A+": "R' F R' B' R F' R' B R'",
  "A-": "R B' R F R' B R F' R",

  "2-triplet-twist": "F' U F U' F BR' F' BR",

  "FL-RB-FR": "Fw U F' U' F Fw' U F U F'",
  "FR-RB-FL": "F U F' U' Fw F' U F U' Fw'",
  "RF-FL-RB": "F' U' F U Fw' F U' F' U Fw",
  "alg-15": "Fw U F' U' Fw' F U' F U F'",
  "alg-16": "Fw' U' F U Fw F' U F' U' F",
  "alg-17": "F' U F U' Fw' F U' F' U Fw",
  "alg-18": "F U' F' U Fw F' U F U' Fw'",
};
