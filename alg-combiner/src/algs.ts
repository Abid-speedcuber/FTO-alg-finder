import type { AlgSet } from "./combine";

// The "big object" of intermediate algs. Write down small building-block
// algorithms here (or live in the UI textarea). Each entry maps a name to a
// move sequence in FTO notation.
export const DEFAULT_INTERMEDIATE_ALGS: AlgSet = {
  hedgeslammer: "F R' F' R",
  sledgehammer: "R' F R F'",
  sexy: "R U R' U'",
  anti_sexy: "U R U' R'",
  trigger_RU: "R U R' U R U R'",
  trigger_FU: "F U F' U F U F'",
  flip_RU: "R U R' U R U' R'",
  flip_FU: "F U F' U F U' F'",
  br_trigger: "BR U BR' U'",
  bl_trigger: "BL U BL' U'",
  insert_front: "R U' R'",
  insert_back: "R U R'",
  insert_left: "F U F'",
  insert_right: "F U' F'",
  comm_RU: "R U R' U'",
  comm_FU: "F' U F U'",
  wide_setup: "Rw U Rw' U'",
  slice_a: "M U M' U'",
  ll_a: "R U R' U' F' U F",
  ll_b: "F U F' U' R' U R",
};
