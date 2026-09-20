// EP and CP cases for the "Set up LL" panel.
//
// Slot names live in the diagram frame documented in llSetup.ts:
//   corners TL, TR, BOT; centers T1/T2 (top strip), R1/R2 (right), L1/L2 (left).
// Cycles are content flow: ["T1", "R1", "L2"] moves T1's piece to R1, R1's to
// L2, and L2's to T1 (this particular cycle is exactly what U does).
// `twists` lists destination slots whose corner ends up twisted.

import { cpPerm, epPerm, type CpOption, type EpOption } from "./llSetup";

export const EP_OPTIONS: EpOption[] = [
  { id: "lunge", label: "lunge", perm: epPerm([["L1", "R1"]]) },
  { id: "left-arm", label: "left arm", perm: epPerm([["L1", "R2"]]) },
  { id: "right-arm", label: "right arm", perm: epPerm([["L2", "R1"]]) },
  { id: "lizard", label: "lizard", perm: epPerm([["L2", "R2"]]) },
  { id: "right-ol", label: "right Ol", perm: epPerm([["T1", "L2", "R1"]]) },
  { id: "lean-right-ol", label: "lean right Ol", perm: epPerm([["T1", "L2", "R2"]]) },
  { id: "right-or", label: "right Or", perm: epPerm([["T1", "R1", "L2"]]) },
  { id: "lean-right-or", label: "lean right Or", perm: epPerm([["T1", "R2", "L2"]]) },
  { id: "left-ol", label: "left Ol", perm: epPerm([["T2", "L1", "R2"]]) },
  { id: "lean-left-ol", label: "lean left Ol", perm: epPerm([["T2", "L2", "R2"]]) },
  { id: "left-or", label: "left Or", perm: epPerm([["T2", "R2", "L1"]]) },
  { id: "lean-left-or", label: "lean left Or", perm: epPerm([["T2", "R2", "L2"]]) },
  { id: "highway", label: "highway", perm: epPerm([["L2", "R1"], ["L1", "R2"]]) },
  { id: "double-lizard", label: "double lizard", perm: epPerm([["T1", "L1"], ["L2", "R2"]]) },
  { id: "pirate", label: "pirate", perm: epPerm([["T1", "L2"], ["L1", "R2"]]) },
  { id: "arms", label: "arms", perm: epPerm([["T2", "L1"], ["L2", "R1"]]) },
  { id: "crossroad", label: "crossroad", perm: epPerm([["T2", "L2"], ["L1", "R1"]]) },
  { id: "left-crawl", label: "left crawl", perm: epPerm([["T2", "L1"], ["L2", "R2"]]) },
  { id: "right-crawl", label: "right crawl", perm: epPerm([["T1", "L1"], ["L2", "R1"]]) },
  { id: "left-climb", label: "left climb", perm: epPerm([["T2", "L2"], ["L1", "R2"]]) },
  { id: "right-climb", label: "right climb", perm: epPerm([["T1", "L2"], ["L1", "R1"]]) },
  { id: "left-whisper", label: "left whisper", perm: epPerm([["T1", "R1"], ["T2", "L2", "R2"]]) },
  { id: "right-whisper", label: "right whisper", perm: epPerm([["T1", "R1", "L1"], ["T2", "R2"]]) },
  { id: "left-duel", label: "left duel", perm: epPerm([["T1", "L1", "R1"], ["T2", "R2"]]) },
  { id: "right-duel", label: "right duel", perm: epPerm([["T1", "R1"], ["T2", "R2", "L2"]]) },
  { id: "left-pinch", label: "left pinch", perm: epPerm([["T1", "L2", "R1"], ["T2", "R2"]]) },
  { id: "right-pinch", label: "right pinch", perm: epPerm([["T1", "R1"], ["T2", "R2", "L1"]]) },
  { id: "left-dynamite", label: "left dynamite", perm: epPerm([["T1", "R1"], ["T2", "L1", "R2"]]) },
  { id: "right-dynamite", label: "right dynamite", perm: epPerm([["T1", "R1", "L2"], ["T2", "R2"]]) },
  { id: "lonely-snail", label: "lonely snail", perm: epPerm([["T1", "R2", "L1"], ["T2", "L2", "R1"]]) },
  { id: "triple-snail", label: "triple snail", perm: epPerm([["T1", "L2", "R1"], ["T2", "R2", "L1"]]) },
  { id: "lonely-lizard", label: "lonely lizard", perm: epPerm([["T1", "L1", "R2"], ["T2", "R1", "L2"]]) },
  { id: "triple-lizard", label: "triple lizard", perm: epPerm([["T1", "R1", "L2"], ["T2", "L1", "R2"]]) },
  { id: "both-ol", label: "both Ol", perm: epPerm([["T1", "L2", "R1"], ["T2", "L1", "R2"]]) },
  { id: "both-or", label: "both Or", perm: epPerm([["T1", "R1", "L2"], ["T2", "R2", "L1"]]) },
];

export const CP_OPTIONS: CpOption[] = [
  { id: "cw", label: "CW", row: 1, perm: cpPerm([["TL", "TR", "BOT"]]), twists: [] },
  { id: "ccw", label: "CCW", row: 1, perm: cpPerm([["TL", "BOT", "TR"]]), twists: [] },
  { id: "twist", label: "Twist", row: 1, perm: cpPerm([]), twists: ["TR", "BOT"] },

  // Row 2: sledges and hedges
  { id: "right-S", label: "right S", row: 2, perm: cpPerm([["TL", "BOT", "TR"]]), twists: ["TR", "BOT"] },
  { id: "right-H", label: "right S", row: 2, perm: cpPerm([["TL", "TR", "BOT"]]), twists: ["TR", "BOT"] },
  { id: "left-S", label: "left S", row: 2, perm: cpPerm([["TL", "BOT", "TR"]]), twists: ["TL", "BOT"] },
  { id: "left-H", label: "left H", row: 2, perm: cpPerm([["TL", "TR", "BOT"]]), twists: ["TL", "BOT"] },
  { id: "back-S", label: "back S", row: 2, perm: cpPerm([["TL", "BOT", "TR"]]), twists: ["TL", "TR"] },
  { id: "back-H", label: "back H", row: 2, perm: cpPerm([["TL", "TR", "BOT"]]), twists: ["TL", "TR"] }
];
