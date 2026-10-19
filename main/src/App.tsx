import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import Pickr from "@simonwep/pickr";
import "@simonwep/pickr/dist/themes/nano.min.css";
import wcaLogo from "./assets/wca.svg";
import githubLogo from "./assets/github.svg";
import AlgCombiner from "./AlgCombiner";
import FtoViewer, { type CenterTargets, type FtoViewerApi } from "./FtoViewer";
import LlSetupSection from "./LlSetupSection";
import { DEFAULT_HOLD, parseExecutionHold, rotateAlgorithm } from "./rotation";

type CubieState = {
  cp: number[];
  co: number[];
  ep: number[];
  uf: number[];
  rl: number[];
};

type ValidationIssue = { message: string; color: number | null };
type ValidationReport = { state: CubieState | null; errors: ValidationIssue[] };

type SolveResult = {
  nodes: number;
  solutions: string[];
};

type SolveLine = {
  kind: string;
  text: string;
};

type PruningCacheStatus = {
  hasTables: boolean;
  hasPruning: boolean;
};

type PruningTableInfo = {
  id: string;
  codename: string;
  moves: string[];
  files: string[];
  bytes: number;
};

type TerminalLine = {
  id: number;
  kind: string;
  text: string;
  createdAt?: number;
};

const moves = [
  "U", "U'", "F", "F'", "BR", "BR'", "BL", "BL'", "D", "D'", "B", "B'",
  "R", "R'", "L", "L'", "Uw", "Uw'", "Fw", "Fw'", "Rw", "Rw'", "Lw", "Lw'", "M", "M'",
  "S", "S'", "E", "E'",
  "(R U R')", "(R U' R')", "(R' U R)", "(R' U' R)",
  "(F U F')", "(F U' F')", "(F' U F)", "(F' U' F)",
];

const defaultInstanceMoves = [
  "U", "U'", "F", "F'", "BR", "BR'", "BL", "BL'", "D", "D'", "B", "B'",
  "R", "R'", "L", "L'", "Fw", "Fw'", "Rw", "Rw'",
];
const defaultMoveSet = new Set(defaultInstanceMoves);
const moreInstanceMoves = moves.filter((move) => !defaultMoveSet.has(move));
const moveChooserNote =
  "All the moves listed here are in CIF. Treat CIF F as EIF R if you wanna generate algs in EIF. Choose the moves you actually use to generate algs. The more moves you choose, the longer the pruning table generation will take, and the weaker the heuristics will be, making all searches take longer on average. You can create new instances later with different move sets, at the cost of disk space for another dedicated set of pruning tables.";

const INSTANCES_KEY = "fto.instances.v1";
const FACE_COLORS_KEY = "fto.faceColors.v1";
const defaultFaceColors = ["#ffff00", "#0000ff", "#ff0000", "#800080", "#ffffff", "#00a050", "#808080", "#ff8800"];
const faceColorSlots = [
  { label: "Top", index: 0 },
  { label: "Bottom", index: 4 },
  { label: "Front", index: 1 },
  { label: "Back", index: 5 },
  { label: "Left", index: 7 },
  { label: "Back-right", index: 2 },
  { label: "Right", index: 6 },
  { label: "Back-left", index: 3 },
];

type Instance = {
  id: string;
  name: string;
  moves: string[];
  banned: string[];
};

type SetupModalState =
  | { kind: "first"; instance: Instance }
  | { kind: "new" }
  | null;

function FaceColorSwatch({
  label,
  color,
  index,
  onChange,
}: {
  label: string;
  color: string;
  index: number;
  onChange: (index: number, color: string) => void;
}) {
  const anchorRef = useRef<HTMLDivElement | null>(null);
  const pickrRef = useRef<Pickr | null>(null);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  function paintPickrButton(pickr: Pickr, nextColor: string) {
    const root = pickr.getRoot() as { button?: HTMLElement };
    const button = root.button;
    if (!button) {
      return;
    }
    button.style.setProperty("--pcr-color", nextColor);
    button.classList.remove("clear");
  }

  function colorToHex(selected: Pickr.HSVaColor | null): string | null {
    if (!selected) {
      return null;
    }
    const rgba = selected.toRGBA();
    return `#${rgba
      .slice(0, 3)
      .map((value) => Math.max(0, Math.min(255, Math.round(value))).toString(16).padStart(2, "0"))
      .join("")}`;
  }

  useEffect(() => {
    const anchor = anchorRef.current;
    if (!anchor || pickrRef.current) {
      return;
    }
    // Pickr replaces its mount node. Keep React's host intact so effect
    // cleanup/setup can run again in StrictMode and during hot reloads.
    const mount = document.createElement("div");
    anchor.appendChild(mount);
    const pickr = Pickr.create({
      el: mount,
      theme: "nano",
      appClass: "fto-pickr-app",
      default: color,
      swatches: null,
      position: "bottom-middle",
      defaultRepresentation: "RGBA",
      components: {
        preview: true,
        opacity: false,
        hue: true,
        interaction: {
          hex: false,
          rgba: false,
          hsla: false,
          hsva: false,
          cmyk: false,
          input: true,
          clear: false,
          save: false,
          cancel: false,
        },
      },
    });
    pickr.on("change", (selected: Pickr.HSVaColor | null) => {
      const nextColor = colorToHex(selected);
      if (nextColor) {
        pickr.applyColor(true);
        paintPickrButton(pickr, nextColor);
        onChangeRef.current(index, nextColor);
      }
    });
    pickr.on("hide", () => {
      const nextColor = colorToHex(pickr.getColor());
      if (nextColor) {
        paintPickrButton(pickr, nextColor);
        onChangeRef.current(index, nextColor);
      }
    });
    pickr.on("show", () => {
      const root = pickr.getRoot() as { button?: HTMLElement };
      root.button?.classList.add("pcr-open");
    });
    pickr.on("hide", () => {
      const root = pickr.getRoot() as { button?: HTMLElement };
      root.button?.classList.remove("pcr-open");
    });
    try {
      pickr.setColor(color, true);
    } catch {
      // Pickr keeps the last valid color if initialization receives bad input.
    }
    paintPickrButton(pickr, color);
    pickr.on("init", () => paintPickrButton(pickr, color));
    const paintFrame = requestAnimationFrame(() => paintPickrButton(pickr, color));
    const paintTimeout = window.setTimeout(() => paintPickrButton(pickr, color), 50);
    pickrRef.current = pickr;
    return () => {
      cancelAnimationFrame(paintFrame);
      window.clearTimeout(paintTimeout);
      pickrRef.current = null;
      pickr.destroyAndRemove();
    };
  }, [index]);

  useEffect(() => {
    if (!pickrRef.current) {
      return;
    }
    pickrRef.current.setColor(color, true);
    paintPickrButton(pickrRef.current, color);
  }, [color]);

  return (
    <div className="scheme-row">
      <span className="scheme-face-label">{label}</span>
      <div className="scheme-pickr-wrap" aria-label={`${label} color`} title={label}>
        <div ref={anchorRef} />
      </div>
    </div>
  );
}

const solved: CubieState = {
  cp: [0, 1, 2, 3, 4, 5],
  co: [0, 0, 0, 0, 0, 0],
  ep: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
  uf: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
  rl: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
};

function emptyCenterTargets(): CenterTargets {
  return {
    uf: [null, null, null, null],
    rl: [null, null, null, null],
    ufSources: [null, null, null, null],
    rlSources: [null, null, null, null],
    rlTopSources: [[], [], [], []],
  };
}

function newInstanceId(): string {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 9)}`;
}

function nextInstanceName(instances: Instance[]): string {
  const used = new Set(instances.map((instance) => instance.name.trim()));
  let index = instances.length + 1;
  while (used.has(`Instance ${index}`)) {
    index += 1;
  }
  return `Instance ${index}`;
}

function pruningProgressPercent(text: string): number {
  const match = text.match(/\((\d+(?:\.\d+)?)%\)/);
  if (!match) {
    return 0;
  }
  return Math.max(0, Math.min(100, Number(match[1])));
}

function pruningProgressName(text: string): string {
  return (
    text.match(/^(?:pruning progress|loading pruning table progress|saving pruning table progress):\s+(.+?)\s+reached/)?.[1] ??
    "pruning"
  );
}

function transitionProgressName(text: string): string {
  return (
    text.match(/^(?:transition progress|loading transition table progress|saving transition table progress):\s+(.+?)\s+\d+\/\d+/)?.[1] ??
    "transition tables"
  );
}

function isProgressLine(text: string): boolean {
  return (
    text.startsWith("pruning progress:") ||
    text.startsWith("loading pruning table progress:") ||
    text.startsWith("saving pruning table progress:") ||
    text.startsWith("transition progress:") ||
    text.startsWith("loading transition table progress:") ||
    text.startsWith("saving transition table progress:")
  );
}

function isGeneratingPruningProgress(text: string): boolean {
  return text.startsWith("pruning progress:");
}

function progressHeading(line: TerminalLine): string {
  if (line.text.startsWith("loading pruning table progress:")) {
    return "loading pruning table...";
  }
  if (line.text.startsWith("saving pruning table progress:")) {
    return "saving pruning table...";
  }
  if (line.text.startsWith("pruning progress:")) {
    return "generating pruning table...";
  }
  if (line.text.startsWith("loading transition table progress:")) {
    return "loading transition tables...";
  }
  if (line.text.startsWith("saving transition table progress:")) {
    return "saving transition tables...";
  }
  return "generating transition tables...";
}

const pruningLineRotationMs = 16_000;
const pruningSlowGateMs = 20_000;
const pruningSlowGatePercent = 60;
const pruningHangTightPercent = 85;
const pruningAnotherTablePercent = 99;
const pruningFirstSlowLine =
  "Progress isn't linear here. The progress gets slower as the depth increases. You might as well grab a popcorn meanwhile";
const pruningSecondSlowLine =
  "Uh-oh, looks like I miscalculated. It will take a lil bit longer than I was hoping. Should still be faster than the previous table tho. You might wanna grab a second popcorn, maybe?";
const pruningExpensiveDoneLine = "Yay, we're done with the exxxpensive table, this one should be faster than the previous one.";
const pruningHangTightLine = "Hang tight, we're almost there.";
const pruningAnotherTableLine = "btw, we have another table left, it wont take this long I promise";

function isExpensivePruningTable(name: string): boolean {
  const normalized = name.toLowerCase();
  return normalized.includes("edge3") || normalized.includes("e3cf3");
}

// FTO history and geometry sources (checked October 2026):
// https://patents.google.com/patent/US4451039A/
// https://patents.google.com/patent/DE3206560A1/de
// https://patents.google.com/patent/US20050006842A1/en
// https://www.pitcherpuzzles.com/the-archives
// https://twistypuzzles.com/app/museum/museum_showitem.php?pkey=1663
// https://twistypuzzles.com/forum/viewtopic.php?p=392974
// https://en.wikipedia.org/wiki/Face_Turning_Octahedron (2018 Bencisco / 2021 RexTO)
// https://ruwix.com/twisty-puzzles/face-turning-octahedron-fto/ (community growth)
// https://www.worldcubeassociation.org/posts/changes-to-the-wca-s-list-of-official-events-june-2026
// https://www.jaapsch.net/puzzles/octaface.htm
// https://math.ucr.edu/home/baez/platonic.html (convex regular polytopes)
// https://math.ucr.edu/home/baez/week155.html (two-coloring)
// https://mathworld.wolfram.com/RegularOctahedron.html (geometry and symmetries)
// Search explanations match fto-core/src/search.rs and fto-core/src/pruning.rs.
const pruningFtoFacts = [
  "FTO was already on paper in 1982: Clarence W. Hewlett Jr. filed a US patent for a face-turning octahedron on February 9 that year; it was granted on May 29, 1984.",
  "Two inventors filed FTO patents just 15 days apart in 1982: Clarence W. Hewlett Jr. in the US on February 9, and Karl Rohrbach in Germany on February 24.",
  "David Pitcher independently reinvented FTO in the early 2000s, building a working mechanism roughly twenty years after the first FTO patents.",
  "An FTO doesn't need eight colors: inventor David Pitcher's original used four, with opposite faces sharing a color. That still distinguishes every piece.",
  "FTO speedsolving got a dedicated method in 2018, when Ben Streeter began developing Bencisco, his method for solving the face-turning octahedron.",
  "Ben Streeter helped turn FTO into a speedsolving event through his Bencisco method and tutorial; rankings and the FTO Fan Club helped spread it during the pandemic.",
  "Before modern FTO hardware, Ben Streeter's 2021 RexTO was a common way of converting a Rex Cube into an FTO shape using 3D-printed extensions.",
  "The first official FTO competition was on January 2, 2027, as announced in June 2026. It is the first event added since Skewb in 2014.",
  "FTO has 42 moving pieces: 6 corners, 12 edges, and 24 centers. More than half the pieces are centers.",
  "FTO has 31,408,133,379,194,880,000,000 reachable positions, treating same-colored centers as identical and ignoring whole-puzzle rotations.",
  "The octahedron is the cube's evil twin, geometrically: the shapes are duals, swapping 6 faces and 8 vertices for 8 faces and 6 vertices.",
  "Join the centers of neighboring faces on a cube and you get an octahedron. Do the same to an octahedron and you get a cube.",
  "A cube and a regular octahedron have the same rotational symmetries: 24 ways to rotate either shape onto itself.",
  "A regular octahedron is two square pyramids joined base to base. The square in the middle is hidden inside the solid.",
  "The octahedron is the only Platonic solid whose faces can alternate between two colors without matching colors sharing an edge.",
  "There are exactly five convex Platonic solids in 3D. The octahedron is one; the cube, tetrahedron, dodecahedron, and icosahedron are the others.",
  "There are six convex regular polytopes in 4D, but only three in every dimension above four: the simplex, hypercube, and cross-polytope.",
  "The octahedron's higher-dimensional family survives in every dimension: the cross-polytopes, duals of the hypercubes.",
  "The 4D cousin of an octahedron is the 16-cell. Its boundary consists of 16 tetrahedra, not octahedra.",
  "A 4D regular polytope called the 24-cell has a boundary made of 24 octahedra. It has no regular counterpart in 3D.",
  "The octahedron has 6 vertices, 12 edges, and 8 faces: 6 - 12 + 8 = 2, an example of Euler's polyhedron formula.",
];

const pruningSearchNotes = [
  "Solver: 5 moves left.\nTable: At least 6 needed.\nResult: skip this branch and everything below it.",
  "Solver: 5 moves left.\nTable: At least 4 needed.\nResult: keep searching; the bound permits a solution but doesn't guarantee one.",
  "Solver: Edges need 5, corners need 4.\nBound: 5, not 9. The same moves can solve both.",
  "A pruning table solves a smaller problem: track some pieces and ignore the rest. The full puzzle needs at least as many moves as that smaller problem.",
  "Pruning tables store lower bounds on distance, not algorithms. The solver uses those bounds to reject branches that cannot fit the move limit.",
  "The pruning tables here are generated with a couple of pieces, not all of them.",
  "One extra move of search means another layer of choices on every surviving branch. Search time grows exponentially with additional allowed moves.",
  "A transition table stores what each move does to a pattern. A pruning table stores how far that pattern is from solved.",
  "Generating a pruning table pays the calculation cost up front. Later solves reuse the stored distances.",
];

const pruningJokes = [
  "The table generates faster when you're not looking. Trust.",
  "This would be finished by now if you were not lookig by the way, this one is on you.",
  "These pruning tables can speed up a search by tens of thousands of times."
];

// Interleave the topics so every wait gets facts, explanations, and a few jokes.
const pruningRotatingLines = Array.from(
  { length: Math.max(pruningFtoFacts.length, pruningSearchNotes.length, pruningJokes.length) },
  (_, index) => [pruningFtoFacts[index], pruningSearchNotes[index], pruningJokes[index]],
).flat().filter((line): line is string => line !== undefined);

type PruningProgressPoint = {
  name: string;
  percent: number;
  createdAt: number;
};

function firstProgressAt(points: PruningProgressPoint[], percent: number): PruningProgressPoint | undefined {
  return points.find((point) => point.percent >= percent);
}

function pruningTableWasSlow(points: PruningProgressPoint[], observedAt?: number): boolean {
  const startedAt = points[0]?.createdAt;
  if (startedAt === undefined) {
    return false;
  }
  const firstAtGate = firstProgressAt(points, pruningSlowGatePercent);
  if (firstAtGate) {
    return firstAtGate.createdAt - startedAt > pruningSlowGateMs;
  }
  const lastPoint = points[points.length - 1];
  return !!lastPoint && (observedAt ?? lastPoint.createdAt) - startedAt >= pruningSlowGateMs;
}

function pickRotatingPruningLine(name: string, slot: number): string {
  // Different tables start at different places; each then visits the whole pool.
  const offset = Array.from(name).reduce((hash, character) => (hash * 31 + character.charCodeAt(0)) >>> 0, 0);
  return pruningRotatingLines[(offset + slot - 1) % pruningRotatingLines.length];
}

function pickPruningOneLiner(
  progressText: string,
  progressHistory: PruningProgressPoint[],
  now: number,
): string | null {
  const currentName = pruningProgressName(progressText);
  const allPoints = progressHistory;
  const tableNames = Array.from(new Set(allPoints.map((point) => point.name)));
  const currentTableIndex = tableNames.indexOf(currentName);
  const currentPoints = allPoints.filter((point) => point.name === currentName);
  const tableStartedAt = currentPoints[0]?.createdAt;
  if (tableStartedAt === undefined) {
    return null;
  }

  const tableElapsedMs = now - tableStartedAt;
  const activeAt = tableStartedAt + pruningSlowGateMs;
  const slowEnoughForBanter = pruningTableWasSlow(currentPoints, now);
  const previousTableName = currentTableIndex > 0 ? tableNames[currentTableIndex - 1] : undefined;
  const previousTableWasSlow = previousTableName
    ? pruningTableWasSlow(allPoints.filter((point) => point.name === previousTableName))
    : false;

  if (!slowEnoughForBanter) {
    if (previousTableWasSlow && tableElapsedMs < pruningSlowGateMs) {
      return pruningExpensiveDoneLine;
    }
    return null;
  }

  const slot = Math.max(0, Math.floor((now - activeAt) / pruningLineRotationMs));
  if (slot === 0) {
    return previousTableWasSlow ? pruningSecondSlowLine : pruningFirstSlowLine;
  }

  const forceCandidates = [
    {
      text: pruningHangTightLine,
      point: firstProgressAt(currentPoints, pruningHangTightPercent),
    },
    {
      text: pruningAnotherTableLine,
      point:
        currentTableIndex === 0 && isExpensivePruningTable(currentName)
          ? firstProgressAt(currentPoints, pruningAnotherTablePercent)
          : undefined,
    },
  ];
  let nextForcedSlot = 1;
  let usedForcedSlots = 0;
  for (const candidate of forceCandidates) {
    if (!candidate.point) {
      continue;
    }
    const conditionSlot = Math.max(1, Math.floor((candidate.point.createdAt - activeAt) / pruningLineRotationMs) + 1);
    const scheduledSlot = Math.max(nextForcedSlot, conditionSlot);
    nextForcedSlot = scheduledSlot + 1;
    if (scheduledSlot < slot) {
      usedForcedSlots += 1;
    }
    if (scheduledSlot === slot) {
      return candidate.text;
    }
  }

  return pickRotatingPruningLine(currentName, slot - usedForcedSlots);
}

function normalizeHexColor(value: string): string | null {
  const trimmed = value.trim();
  const match = /^#?([0-9a-fA-F]{6})$/.exec(trimmed);
  return match ? `#${match[1].toLowerCase()}` : null;
}

function bootstrapInstances(): { instances: Instance[]; activeId: string; firstRun: boolean } {
  try {
    const raw = localStorage.getItem(INSTANCES_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as { instances?: unknown[]; activeId?: unknown };
      if (Array.isArray(parsed.instances)) {
        const instances = parsed.instances.filter(
          (item): item is Instance =>
            !!item &&
            typeof (item as Instance).id === "string" &&
            typeof (item as Instance).name === "string" &&
            Array.isArray((item as Instance).moves) &&
            Array.isArray((item as Instance).banned),
        );
        if (instances.length > 0) {
          const ids = new Set(instances.map((instance) => instance.id));
          const activeId =
            typeof parsed.activeId === "string" && ids.has(parsed.activeId)
              ? parsed.activeId
              : instances[0].id;
          return { instances, activeId, firstRun: false };
        }
      }
    }
  } catch {
    // fall through to fresh bootstrap
  }
  const id = "default";
  return {
    instances: [{ id, name: "Default", moves: [...defaultInstanceMoves], banned: [] }],
    activeId: id,
    firstRun: true,
  };
}

function InstanceSetupModal({
  title,
  subtitle,
  initialName,
  initialMoves,
  confirmLabel,
  note,
  cancellable = true,
  onSave,
  onCancel,
}: {
  title: string;
  subtitle?: string;
  initialName: string;
  initialMoves: string[];
  confirmLabel: string;
  note?: string;
  cancellable?: boolean;
  onSave: (name: string, selected: string[]) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(initialName);
  const [selected, setSelected] = useState<Set<string>>(() => new Set(initialMoves));
  const [moreOpen, setMoreOpen] = useState(false);
  const visibleMoves = moreOpen ? moves : defaultInstanceMoves;
  const activeMoves = visibleMoves.filter((move) => selected.has(move));

  const selectVisibleMoves = (selectAll: boolean) => {
    setSelected((current) => {
      const next = new Set(current);
      for (const move of visibleMoves) {
        if (selectAll) {
          next.add(move);
        } else {
          next.delete(move);
        }
      }
      return next;
    });
  };

  const toggleMove = (move: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(move)) {
        next.delete(move);
      } else {
        next.add(move);
      }
      return next;
    });
  };

  const canSave = activeMoves.length > 0;
  const renderMoveButton = (move: string) => (
    <button
      key={move}
      className={selected.has(move) ? "move-toggle" : "move-toggle banned"}
      onClick={() => toggleMove(move)}
    >
      {move}
    </button>
  );

  return (
    <div className="modal-backdrop" onClick={cancellable ? onCancel : undefined}>
      <div className="modal instance-setup-modal" onClick={(event) => event.stopPropagation()}>
        <h2>{title}</h2>
        {subtitle ? <p className="modal-subtitle">{subtitle}</p> : null}
        <div className="instance-setup-body">
          <label className="field">
            <span>Name</span>
            <input
              value={name}
              onChange={(event) => setName(event.target.value)}
              spellCheck={false}
              autoComplete="off"
              placeholder="Instance name"
            />
          </label>
          <div className="modal-moves-heading">
            <div className="field">
              <span>Moves this instance uses</span>
            </div>
            <div className="mini-actions">
              <button className="secondary" onClick={() => selectVisibleMoves(true)}>
                All
              </button>
              <button className="secondary" onClick={() => selectVisibleMoves(false)}>
                None
              </button>
            </div>
          </div>
          <div className="modal-move-grid">{defaultInstanceMoves.map(renderMoveButton)}</div>
          <button className="more-moves-toggle" onClick={() => setMoreOpen((open) => !open)}>
            <span>More moves</span>
            <span className="more-moves-icon" aria-hidden="true">
              <svg viewBox="0 0 20 20" focusable="false">
                <path d={moreOpen ? "M5.5 12.2 10 7.8l4.5 4.4" : "M5.5 7.8 10 12.2l4.5-4.4"} />
              </svg>
            </span>
          </button>
          {moreOpen ? (
            <div className="modal-move-grid more-move-grid">{moreInstanceMoves.map(renderMoveButton)}</div>
          ) : null}
          {note ? <p className="move-chooser-note">{note}</p> : null}
        </div>
        <div className="modal-footer">
          {cancellable ? (
            <button className="secondary" onClick={onCancel}>
              Cancel
            </button>
          ) : null}
          <button
            className="primary"
            disabled={!canSave}
            onClick={() => onSave(name, activeMoves)}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

function App() {
  const [mode, setMode] = useState<"solver" | "combiner">("solver");
  const [boot] = useState(bootstrapInstances);
  const [instances, setInstances] = useState<Instance[]>(boot.instances);
  const [activeId, setActiveId] = useState(boot.activeId);
  const [setupModal, setSetupModal] = useState<SetupModalState>(
    boot.firstRun ? { kind: "first", instance: boot.instances[0] } : null,
  );
  const [menuOpen, setMenuOpen] = useState(false);
  const [appMenuOpen, setAppMenuOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [pruningOpen, setPruningOpen] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [storageOpen, setStorageOpen] = useState(false);
  const [resetConfirmOpen, setResetConfirmOpen] = useState(false);
  const [showDebugInfo, setShowDebugInfo] = useState(false);
  const [deleteInstanceConfirm, setDeleteInstanceConfirm] = useState<Instance | null>(null);
  const [deleteTableConfirm, setDeleteTableConfirm] = useState<PruningTableInfo | null>(null);
  const [faceColors, setFaceColors] = useState<string[]>(() => {
    try {
      const parsed = JSON.parse(localStorage.getItem(FACE_COLORS_KEY) || "null");
      if (Array.isArray(parsed) && parsed.length === defaultFaceColors.length) {
        return parsed.map(String);
      }
    } catch {
      // fall through
    }
    return defaultFaceColors;
  });
  const [cacheStatus, setCacheStatus] = useState<PruningCacheStatus>({ hasTables: false, hasPruning: false });
  const [pruningTables, setPruningTables] = useState<PruningTableInfo[]>([]);
  const [contextMenu, setContextMenu] = useState<{ id: string; x: number; y: number } | null>(null);

  const [setup, setSetup] = useState("");
  const [inputMode, setInputMode] = useState<"setup" | "alg">("setup");
  const [applySignal, setApplySignal] = useState(0);
  const [facelets, setFacelets] = useState<number[]>([]);
  const [centerTargets, setCenterTargets] = useState<CenterTargets>(emptyCenterTargets);
  const [cubieState, setCubieState] = useState<CubieState | null>(solved);
  const [stateErrors, setStateErrors] = useState<ValidationIssue[]>([]);
  const [stateErrorsOpen, setStateErrorsOpen] = useState(false);
  const stateErrorsDialog = useRef<HTMLDialogElement | null>(null);
  const [depth, setDepth] = useState("");
  const [all, setAll] = useState(true);
  const [restrictedPruning, setRestrictedPruning] = useState(false);
  const [lastLayerMode, setLastLayerMode] = useState(false);
  const [pruningLastLayerMode, setPruningLastLayerMode] = useState(false);
  const viewerApiRef = useRef<FtoViewerApi | null>(null);
  const [threads, setThreads] = useState("1");
  const [status, setStatus] = useState("Idle");
  const [result, setResult] = useState<SolveResult | null>(null);
  const [running, setRunning] = useState(false);
  const [terminal, setTerminal] = useState<TerminalLine[]>([]);
  const [rotationInput, setRotationInput] = useState("U,F");
  const [executionHold, setExecutionHold] = useState(DEFAULT_HOLD);
  const [pruningProgressHistory, setPruningProgressHistory] = useState<PruningProgressPoint[]>([]);
  const [now, setNow] = useState(() => Date.now());
  const [pruningStartedAt, setPruningStartedAt] = useState<number | null>(null);
  const validationRun = useRef(0);
  const lineId = useRef(0);
  const streamedSolutions = useRef(new Set<string>());
  const terminalRef = useRef<HTMLDivElement | null>(null);
  const pruningStartedAtRef = useRef<number | null>(null);
  const modalOpen =
    stateErrorsOpen ||
    settingsOpen ||
    pruningOpen ||
    storageOpen ||
    aboutOpen ||
    resetConfirmOpen ||
    !!deleteInstanceConfirm ||
    !!deleteTableConfirm ||
    !!setupModal;

  const activeInstance = instances.find((instance) => instance.id === activeId) ?? instances[0];
  const contextInstance = contextMenu
    ? instances.find((instance) => instance.id === contextMenu.id)
    : undefined;
  const bannedSet = useMemo(() => new Set(activeInstance.banned), [activeInstance]);
  const allowedMoves = useMemo(
    () => activeInstance.moves.filter((move) => !bannedSet.has(move)),
    [activeInstance, bannedSet],
  );
  const displayTerminal = useMemo(() => {
    if (showDebugInfo) {
      return terminal;
    }
    const lastTerminalLine = terminal[terminal.length - 1];
    if (!result?.solutions.length && lastTerminalLine?.kind === "cancel") {
      return [lastTerminalLine];
    }
    if (!result?.solutions.length && lastTerminalLine?.kind === "error") {
      return [lastTerminalLine];
    }
    if (result?.solutions.length) {
      const foundLine = [...terminal]
        .reverse()
        .find((line) => /^found solution at depth \d+/.test(line.text));
      const depth = foundLine?.text.match(/\d+/)?.[0];
      const solutions = Array.from(
        new Set(
          terminal
            .filter((line) => line.kind === "solution")
            .map((line) => line.text)
            .concat(result.solutions),
        ),
      );
      return [
        {
          id: -2,
          kind: "info",
          text: depth ? `solution found at depth ${depth}` : "solution found",
        },
        ...solutions.map((solution, index) => ({
          id: -1000 - index,
          kind: "solution",
          text: solution,
        })),
        ...(lastTerminalLine?.kind === "cancel" || lastTerminalLine?.kind === "error"
          ? [lastTerminalLine]
          : []),
      ];
    }
    const lines: TerminalLine[] = [];
    const currentDepth = running
      ? [...terminal]
          .reverse()
          .find((line) => line.kind === "search" && line.text.startsWith("searching depth"))
      : undefined;
    const currentProgress = [...terminal]
      .reverse()
      .find((line) => line.kind === "progress" && isProgressLine(line.text));
    if (running && !currentDepth && currentProgress) {
      const isPruningProgress = isGeneratingPruningProgress(currentProgress.text);
      lines.push({
        id: isPruningProgress ? -3 : -4,
        kind: "info",
        text: progressHeading(currentProgress),
      });
      lines.push(currentProgress);
      if (isPruningProgress) {
        const pruningOneLiner = pickPruningOneLiner(currentProgress.text, pruningProgressHistory, now);
        if (pruningOneLiner) {
          lines.push({
            id: -5,
            kind: "info",
            text: pruningOneLiner,
          });
        }
      }
    } else if (
      running &&
      !currentDepth &&
      terminal.some((line) =>
        line.text.includes("transition") || line.text.includes("cached pruning tables") || line.text.includes("LL pruning tables")
      )
    ) {
      const latestTransition = [...terminal]
        .reverse()
        .find((line) =>
          line.text.includes("transition") || line.text.includes("cached pruning tables") || line.text.includes("LL pruning tables")
        );
      lines.push(latestTransition ?? { id: -1, kind: "info", text: "loading transition tables...." });
    } else if (
      running &&
      !currentDepth &&
      terminal.some((line) => line.text.includes("loading pruning") || line.text.includes("using cached pruning"))
    ) {
      const latestPruning = [...terminal]
        .reverse()
        .find((line) => line.text.includes("loading pruning") || line.text.includes("using cached pruning"));
      lines.push(latestPruning ?? { id: -1, kind: "info", text: "loading pruning tables...." });
    }
    if (currentDepth) {
      lines.push(currentDepth);
    }
    const streamedSolutions = terminal.filter((line) => line.kind === "solution");
    if (streamedSolutions.length > 0) {
      lines.push(...streamedSolutions);
    }
    return lines;
  }, [now, pruningProgressHistory, result, running, showDebugInfo, terminal]);

  useEffect(() => {
    try {
      localStorage.setItem(INSTANCES_KEY, JSON.stringify({ instances, activeId }));
    } catch {
      // storage unavailable; keep in-memory instances
    }
  }, [instances, activeId]);

  useEffect(() => {
    try {
      localStorage.setItem(FACE_COLORS_KEY, JSON.stringify(faceColors));
    } catch {
      // storage unavailable; keep in-memory colors
    }
  }, [faceColors]);

  const refreshCacheStatus = useCallback(() => {
    invoke<PruningCacheStatus>("pruning_cache_status")
      .then(setCacheStatus)
      .catch(() => setCacheStatus({ hasTables: false, hasPruning: false }));
  }, []);

  const refreshPruningTables = useCallback(() => {
    invoke<PruningTableInfo[]>("list_pruning_tables")
      .then(setPruningTables)
      .catch(() => setPruningTables([]));
  }, []);

  useEffect(() => {
    refreshCacheStatus();
  }, [refreshCacheStatus]);

  useEffect(() => {
    if (!modalOpen) {
      return;
    }
    const previousOverflow = document.body.style.overflow;
    const previousPaddingRight = document.body.style.paddingRight;
    const scrollbarWidth = window.innerWidth - document.documentElement.clientWidth;
    const currentPaddingRight = Number.parseFloat(window.getComputedStyle(document.body).paddingRight) || 0;
    document.body.style.overflow = "hidden";
    if (scrollbarWidth > 0) {
      document.body.style.paddingRight = `${currentPaddingRight + scrollbarWidth}px`;
    }
    return () => {
      document.body.style.overflow = previousOverflow;
      document.body.style.paddingRight = previousPaddingRight;
    };
  }, [modalOpen]);

  const handleFacelets = useCallback((nextFacelets: number[]) => {
    setFacelets(nextFacelets);
  }, []);

  const appendLine = useCallback((kind: string, text: string, createdAt = Date.now()) => {
    if (kind === "solution") {
      streamedSolutions.current.add(text);
    }
    if (kind === "cancel" && text === "solve cancelled") {
      text = "Search stopped early.";
    }
    const id = ++lineId.current;
    setTerminal((lines) => {
      const next = [...lines, { id, kind, text, createdAt }];
      return next.length > 500 ? next.slice(next.length - 500) : next;
    });
  }, []);

  const clearTerminal = useCallback(() => {
    streamedSolutions.current.clear();
    setTerminal([]);
  }, []);

  const retainStreamedSolutions = useCallback(() => {
    const solutions = Array.from(streamedSolutions.current);
    if (solutions.length) {
      setResult((previous) => ({ nodes: previous?.nodes ?? 0, solutions }));
    }
  }, []);

  const resetPruningTimer = useCallback(() => {
    pruningStartedAtRef.current = null;
    setPruningStartedAt(null);
    setPruningProgressHistory([]);
    setNow(Date.now());
  }, []);

  useEffect(() => {
    if (stateErrorsOpen) {
      stateErrorsDialog.current?.showModal();
    }
  }, [stateErrorsOpen]);

  useEffect(() => {
    if (facelets.length !== 72) {
      return;
    }
    const run = ++validationRun.current;
    invoke<ValidationReport>("validate_facelets", { facelets })
      .then((report) => {
        if (run !== validationRun.current) {
          return;
        }
        setCubieState(report.state);
        setStateErrors(report.errors);
      })
      .catch((error) => {
        if (run !== validationRun.current) {
          return;
        }
        setCubieState(null);
        setStateErrors([{ message: String(error), color: null }]);
      });
  }, [facelets]);

  useEffect(() => {
    if (!running || pruningStartedAt === null) {
      return;
    }
    const interval = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(interval);
  }, [pruningStartedAt, running]);

  useEffect(() => {
    let disposed = false;
    let unlisteners: (() => void)[] = [];
    (async () => {
      const fns = await Promise.all([
        listen<SolveLine>("solve-line", (event) => {
          const createdAt = Date.now();
          if (
            event.payload.kind === "progress" &&
            isGeneratingPruningProgress(event.payload.text)
          ) {
            if (pruningStartedAtRef.current === null) {
              pruningStartedAtRef.current = createdAt;
              setPruningStartedAt(createdAt);
            }
            setNow(createdAt);
            setPruningProgressHistory((points) => [
              ...points,
              {
                name: pruningProgressName(event.payload.text),
                percent: pruningProgressPercent(event.payload.text),
                createdAt,
              },
            ]);
          }
          appendLine(event.payload.kind, event.payload.text, createdAt);
        }),
        listen<SolveResult>("solve-result", (event) => {
          setResult(event.payload);
          setRunning(false);
          resetPruningTimer();
          setStatus(event.payload.solutions.length ? "Done" : "No solution");
          refreshCacheStatus();
        }),
        listen<string>("solve-error", (event) => {
          retainStreamedSolutions();
          appendLine("error", `error: ${event.payload}`);
          setRunning(false);
          resetPruningTimer();
          setStatus(String(event.payload));
          refreshCacheStatus();
        }),
        listen("solve-cancelled", () => {
          retainStreamedSolutions();
          setTerminal((lines) => {
            if (lines[lines.length - 1]?.kind === "cancel") {
              return lines;
            }
            const id = ++lineId.current;
            const next = [...lines, { id, kind: "cancel", text: "Search stopped early." }];
            return next.length > 500 ? next.slice(next.length - 500) : next;
          });
          setRunning(false);
          resetPruningTimer();
          setStatus("Stopped");
          refreshCacheStatus();
        }),
        listen("pruning-generated", () => {
          setRunning(false);
          resetPruningTimer();
          setStatus("Pruning table generated");
          refreshCacheStatus();
          refreshPruningTables();
        }),
      ]);
      if (disposed) {
        fns.forEach((fn) => fn());
      } else {
        unlisteners = fns;
      }
    })();
    return () => {
      disposed = true;
      unlisteners.forEach((fn) => fn());
    };
  }, [appendLine, refreshCacheStatus, refreshPruningTables, resetPruningTimer, retainStreamedSolutions]);

  useEffect(() => {
    if (terminalRef.current) {
      terminalRef.current.scrollTop = terminalRef.current.scrollHeight;
    }
  }, [displayTerminal]);

  function updateInstance(id: string, patch: Partial<Instance>) {
    setInstances((list) => list.map((instance) => (instance.id === id ? { ...instance, ...patch } : instance)));
  }

  function toggleMove(move: string) {
    const id = activeInstance.id;
    setInstances((list) =>
      list.map((instance) => {
        if (instance.id !== id) {
          return instance;
        }
        const banned = instance.banned.includes(move)
          ? instance.banned.filter((entry) => entry !== move)
          : [...instance.banned, move];
        return { ...instance, banned };
      }),
    );
  }

  function allowAllMoves() {
    updateInstance(activeInstance.id, { banned: [] });
  }

  function invertMoveSelection() {
    updateInstance(activeInstance.id, {
      banned: activeInstance.moves.filter((move) => !activeInstance.banned.includes(move)),
    });
  }

  function createInstance(name: string, selected: string[]) {
    const trimmedName = name.trim();
    const instance: Instance = {
      id: newInstanceId(),
      name: trimmedName || nextInstanceName(instances),
      moves: selected,
      banned: [],
    };
    setInstances((list) => [...list, instance]);
    setActiveId(instance.id);
  }

  function deleteInstance(id: string) {
    const deleted = instances.find((instance) => instance.id === id);
    const fallbackInstance: Instance = {
      id: deleted?.id ?? "default",
      name: deleted?.name || "Default",
      moves: deleted?.moves.length ? deleted.moves : [...defaultInstanceMoves],
      banned: [],
    };
    setInstances((list) => {
      const next = list.filter((instance) => instance.id !== id);
      return next.length > 0 ? next : [fallbackInstance];
    });
    setActiveId((current) => {
      if (current !== id) {
        return current;
      }
      return instances.find((instance) => instance.id !== id)?.id ?? fallbackInstance.id;
    });
    if (instances.length <= 1) {
      setActiveId(fallbackInstance.id);
      setSetupModal({ kind: "first", instance: fallbackInstance });
    }
  }

  function openContextMenu(instanceId: string, event: React.MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    setMenuOpen(true);
    setContextMenu({ id: instanceId, x: event.clientX, y: event.clientY });
  }

  function closeOverlays() {
    setMenuOpen(false);
    setAppMenuOpen(false);
    setContextMenu(null);
  }

  async function solve() {
    if (!cubieState || running) {
      return;
    }
    const trimmedDepth = depth.trim();
    if (trimmedDepth && !/^\d+$/.test(trimmedDepth)) {
      setStatus("Depth must be empty or numeric");
      return;
    }
    setRunning(true);
    setStatus("Solving");
    setResult(null);
    resetPruningTimer();
    clearTerminal();
    appendLine(
      "info",
      `solve (instance=${activeInstance.name} depth=${trimmedDepth || "auto"} all=${all ? "on" : "off"} threads=${threads})`,
    );
    try {
      await invoke("solve_fto", {
        request: {
          scramble: null,
          state: null,
          facelets,
          centerTargets,
          allowedMoves,
          instanceMoves: activeInstance.moves,
          maxDepth: trimmedDepth ? Number(trimmedDepth) : null,
          findAll: all,
          restrictedPruning,
          miniPruning: false,
          lastLayerMode,
          threads: Number(threads),
        },
      });
    } catch (error) {
      appendLine("error", `error: ${String(error)}`);
      setStatus(String(error));
      setRunning(false);
      resetPruningTimer();
      refreshCacheStatus();
    }
  }

  async function stopSolve() {
    await invoke("stop_solve");
    setStatus("Stopping");
  }

  async function generatePruningTable() {
    if (running || allowedMoves.length === 0) {
      return;
    }
    setPruningOpen(false);
    setRunning(true);
    setStatus("Generating pruning table");
    setResult(null);
    resetPruningTimer();
    clearTerminal();
    appendLine(
      "info",
      `generate pruning (moves=${restrictedPruning ? "selected" : "instance"} last-layer=${
        pruningLastLayerMode ? "on" : "off"
      } threads=${threads})`,
    );
    try {
      await invoke("generate_pruning_table", {
        request: {
          facelets,
          centerTargets,
          allowedMoves,
          instanceMoves: activeInstance.moves,
          selectedMovesOnly: restrictedPruning,
          lastLayerMode: pruningLastLayerMode,
          definedPiecesOnly: false,
          threads: Number(threads),
        },
      });
    } catch (error) {
      appendLine("error", `error: ${String(error)}`);
      setStatus(String(error));
      setRunning(false);
      resetPruningTimer();
      refreshCacheStatus();
    }
  }

  async function unloadPruningTable() {
    try {
      await invoke("unload_pruning_table");
      setStatus("Pruning table unloaded");
      appendLine("info", "unloaded pruning table from RAM");
      refreshCacheStatus();
    } catch (error) {
      appendLine("error", `error: ${String(error)}`);
      setStatus(String(error));
    }
  }

  async function deletePruningTable(table: PruningTableInfo) {
    await invoke("delete_pruning_table", { id: table.id });
    setDeleteTableConfirm(null);
    refreshPruningTables();
    refreshCacheStatus();
  }

  async function resetAllAppData() {
    try {
      await invoke("reset_app_data");
    } catch (error) {
      appendLine("error", `error: ${String(error)}`);
    }
    try {
      localStorage.removeItem(INSTANCES_KEY);
      localStorage.removeItem(FACE_COLORS_KEY);
    } catch {
      // storage unavailable; keep going with in-memory reset
    }
    const instance: Instance = { id: "default", name: "Default", moves: [...defaultInstanceMoves], banned: [] };
    setInstances([instance]);
    setActiveId(instance.id);
    setFaceColors(defaultFaceColors);
    setSetup("");
    setInputMode("setup");
    setApplySignal((current) => current + 1);
    setDepth("");
    setAll(true);
    setRestrictedPruning(false);
    setLastLayerMode(false);
    setPruningLastLayerMode(false);
    setResult(null);
    clearTerminal();
    setStatus("App data reset");
    setResetConfirmOpen(false);
    setSettingsOpen(false);
    setSetupModal({ kind: "first", instance });
    refreshCacheStatus();
    refreshPruningTables();
  }

  function updateFaceColor(index: number, color: string) {
    const normalized = normalizeHexColor(color);
    if (!normalized) {
      return;
    }
    setFaceColors((colors) => colors.map((current, i) => (i === index ? normalized : current)));
  }

  function closeStateErrors() {
    stateErrorsDialog.current?.close();
    setStateErrorsOpen(false);
  }

  return (
    <main>
      <header className="app-header">
        <div>
          <h1>{mode === "combiner" ? "FTO Alg Combiner" : "FTO Alg Finder"}</h1>
          <p>by Abid &amp; Matt</p>
        </div>
        <div className="topbar-right">
          <div className="settings-menu">
            <button
              type="button"
              className="icon-trigger vertical"
              aria-label="Open menu"
              title="Menu"
              onClick={() => setAppMenuOpen((open) => !open)}
            >
              ⋮
            </button>
            {appMenuOpen ? (
              <>
                <div className="menu-backdrop" onClick={closeOverlays} />
                <div className="settings-panel app-menu-panel">
                  <button
                    className="menu-command"
                    onClick={() => {
                      setMode(mode === "solver" ? "combiner" : "solver");
                      closeOverlays();
                    }}
                  >
                    {mode === "solver" ? "Switch to alg combiner" : "Switch to solver mode"}
                  </button>
                  <button
                    className="menu-command"
                    onClick={() => {
                      setPruningOpen(true);
                      closeOverlays();
                    }}
                  >
                    Pruning
                  </button>
                  <button
                    className="menu-command"
                    onClick={() => {
                      setSettingsOpen(true);
                      closeOverlays();
                    }}
                  >
                    Settings
                  </button>
                  <button
                    className="menu-command"
                    onClick={() => {
                      setAboutOpen(true);
                      closeOverlays();
                    }}
                  >
                    About
                  </button>
                </div>
              </>
            ) : null}
          </div>
        </div>
      </header>

      {mode === "solver" && contextMenu ? (
        <div className="context-menu" style={{ left: contextMenu.x, top: contextMenu.y }}>
          <button
            className="context-delete"
            disabled={!contextInstance}
            onClick={() => {
              if (contextInstance) {
                setDeleteInstanceConfirm(contextInstance);
              }
              closeOverlays();
            }}
          >
            Delete {contextInstance?.name ?? "instance"}
          </button>
        </div>
      ) : null}

      {stateErrorsOpen ? (
        <dialog
          ref={stateErrorsDialog}
          className="modal validation-modal"
          aria-labelledby="state-errors-title"
          onClose={() => setStateErrorsOpen(false)}
          onCancel={(event) => { event.preventDefault(); closeStateErrors(); }}
          onClick={(event) => {
            const bounds = event.currentTarget.getBoundingClientRect();
            if (event.clientX < bounds.left || event.clientX > bounds.right ||
                event.clientY < bounds.top || event.clientY > bounds.bottom) {
              closeStateErrors();
            }
          }}
        >
          <div className="modal-title-row">
            <h2 id="state-errors-title">Invalid state</h2>
            <button className="validation-close" autoFocus onClick={closeStateErrors}>Close</button>
          </div>
          {stateErrors.length ? (
            <>
              <p className="validation-summary">{stateErrors.length} {stateErrors.length === 1 ? "problem needs" : "problems need"} attention.</p>
              <ul className="validation-errors">
                {stateErrors.map((error, index) => (
                  <li key={index}>
                    {error.color !== null ? <>
                      Color{" "}
                      <span className="validation-color" style={{ backgroundColor: faceColors[error.color] }}
                        role="img" aria-label={`${faceColorSlots.find((slot) => slot.index === error.color)?.label ?? "Face"} color ${faceColors[error.color]}`}
                        title={`${faceColorSlots.find((slot) => slot.index === error.color)?.label ?? "Face"} (${faceColors[error.color]})`} />
                      {" "}
                    </> : null}
                    {error.message}
                  </li>
                ))}
              </ul>
            </>
          ) : <p>The puzzle is now valid.</p>}
        </dialog>
      ) : null}

      {settingsOpen ? (
        <div className="modal-backdrop" onClick={() => setSettingsOpen(false)}>
          <div className="modal settings-modal" onClick={(event) => event.stopPropagation()}>
            <div className="modal-title-row">
              <h2>Settings</h2>
              <button className="secondary" onClick={() => setSettingsOpen(false)}>Close</button>
            </div>
            <label className="settings-toggle modal-toggle">
              <span>Show debug info</span>
              <input
                type="checkbox"
                checked={showDebugInfo}
                onChange={(event) => setShowDebugInfo(event.target.checked)}
              />
            </label>
            <div className="settings-group">
              <div className="settings-group-title">
                <h3>Face colors</h3>
                <button className="secondary compact" onClick={() => setFaceColors(defaultFaceColors)}>
                  Reset to default
                </button>
              </div>
              <div className="settings-swatch-grid">
                {faceColorSlots.map((slot) => (
                  <FaceColorSwatch
                    key={slot.index}
                    label={slot.label}
                    color={faceColors[slot.index]}
                    index={slot.index}
                    onChange={updateFaceColor}
                  />
                ))}
              </div>
            </div>
            <button
              className="secondary"
              onClick={() => {
                refreshPruningTables();
                setStorageOpen(true);
              }}
            >
              Manage storage
            </button>
            <button className="danger" onClick={() => setResetConfirmOpen(true)}>
              Reset all app data
            </button>
          </div>
        </div>
      ) : null}

      {pruningOpen ? (
        <div className="modal-backdrop" onClick={() => setPruningOpen(false)}>
          <div className="modal pruning-modal" onClick={(event) => event.stopPropagation()}>
            <div className="modal-title-row">
              <h2>Pruning</h2>
              <button className="secondary" onClick={() => setPruningOpen(false)}>Close</button>
            </div>
            <div className="option-list modal-option-list">
              <label className="check modal-check">
                <input
                  type="checkbox"
                  checked={restrictedPruning}
                  onChange={(event) => setRestrictedPruning(event.target.checked)}
                />
                Use only the selected moves to generate the pruning table
              </label>
              <label className="check modal-check">
                <input
                  type="checkbox"
                  checked={pruningLastLayerMode}
                  onChange={(event) => setPruningLastLayerMode(event.target.checked)}
                />
                Use last layer mode
              </label>
            </div>
            <div className="modal-footer">
              <button className="primary" onClick={generatePruningTable} disabled={running || allowedMoves.length === 0}>
                Generate pruning table
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {storageOpen ? (
        <div className="modal-backdrop" onClick={() => setStorageOpen(false)}>
          <div className="modal storage-modal" onClick={(event) => event.stopPropagation()}>
            <div className="modal-title-row">
              <h2>Storage</h2>
              <button className="secondary" onClick={() => setStorageOpen(false)}>Close</button>
            </div>
            {pruningTables.length === 0 ? (
              <div className="hint">No pruning tables found on disk.</div>
            ) : (
              <div className="storage-list">
                {pruningTables.map((table) => (
                  <div key={table.id} className="storage-row">
                    <div>
                      <div className="storage-title">{table.codename}</div>
                      <div className="storage-sub">
                        {table.moves.join(" ")} · {(table.bytes / (1024 * 1024)).toFixed(1)} MiB
                      </div>
                    </div>
                    <button className="secondary" onClick={() => setDeleteTableConfirm(table)}>
                      Delete
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      ) : null}

      {deleteTableConfirm ? (
        <div className="modal-backdrop" onClick={() => setDeleteTableConfirm(null)}>
          <div className="modal confirm-modal" onClick={(event) => event.stopPropagation()}>
            <h2>Delete pruning table?</h2>
            <p className="modal-subtitle">
              This removes {deleteTableConfirm.codename} from disk. It can be regenerated later, but the next solve
              using these moves may need to rebuild it.
            </p>
            <div className="storage-row confirm-summary">
              <div>
                <div className="storage-title">{deleteTableConfirm.codename}</div>
                <div className="storage-sub">
                  {deleteTableConfirm.moves.join(" ")} · {(deleteTableConfirm.bytes / (1024 * 1024)).toFixed(1)} MiB
                </div>
              </div>
            </div>
            <div className="modal-footer">
              <button className="secondary" onClick={() => setDeleteTableConfirm(null)}>
                Cancel
              </button>
              <button className="danger" onClick={() => deletePruningTable(deleteTableConfirm)}>
                Delete
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {deleteInstanceConfirm ? (
        <div className="modal-backdrop" onClick={() => setDeleteInstanceConfirm(null)}>
          <div className="modal confirm-modal" onClick={(event) => event.stopPropagation()}>
            <h2>Delete instance?</h2>
            <p className="modal-subtitle">
              Do you really want to delete {deleteInstanceConfirm.name}? Pruning tables related to this instance will
              not be automatically deleted. To delete them, go to Settings &gt; Manage storage.
            </p>
            <div className="storage-row confirm-summary">
              <div>
                <div className="storage-title">{deleteInstanceConfirm.name}</div>
                <div className="storage-sub">{deleteInstanceConfirm.moves.length} moves</div>
              </div>
            </div>
            <div className="modal-footer">
              <button className="secondary" onClick={() => setDeleteInstanceConfirm(null)}>
                Cancel
              </button>
              <button
                className="danger"
                onClick={() => {
                  deleteInstance(deleteInstanceConfirm.id);
                  setDeleteInstanceConfirm(null);
                }}
              >
                Delete instance
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {resetConfirmOpen ? (
        <div className="modal-backdrop" onClick={() => setResetConfirmOpen(false)}>
          <div className="modal confirm-modal" onClick={(event) => event.stopPropagation()}>
            <h2>Reset all app data?</h2>
            <p className="modal-subtitle">
              This clears local settings, instances, generated table caches, and anything currently loaded in RAM.
            </p>
            <div className="modal-footer">
              <button className="secondary" onClick={() => setResetConfirmOpen(false)}>
                Cancel
              </button>
              <button className="danger" onClick={resetAllAppData}>
                Yes, reset
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {aboutOpen ? (
        <div className="modal-backdrop" onClick={() => setAboutOpen(false)}>
          <div className="modal about-modal" onClick={(event) => event.stopPropagation()}>
            <div className="modal-title-row">
              <h2>About</h2>
              <button className="secondary" onClick={() => setAboutOpen(false)}>Close</button>
            </div>
            <p>Created by Abid and Matt.</p>
            <div className="about-authors">
              {[
                { name: "Abid", wcaId: "2024ASHR02", github: "https://github.com/Abid-speedcuber" },
                { name: "Matt", wcaId: "2023MAOS01", github: "https://github.com/Mattttttttttttttttttttttttttttttt" },
              ].map((author) => (
                <div className="about-author" key={author.name}>
                  <strong>{author.name}</strong>
                  <div className="about-links">
                    <a
                      href={`https://www.worldcubeassociation.org/persons/${author.wcaId}`}
                      target="_blank"
                      rel="noreferrer"
                      aria-label={`${author.name}'s WCA profile (${author.wcaId})`}
                    >
                      <img src={wcaLogo} alt="" width="24" height="24" />
                      {author.wcaId}
                    </a>
                    <a href={author.github} target="_blank" rel="noreferrer" aria-label={`${author.name}'s GitHub profile`}>
                      <img src={githubLogo} alt="" width="24" height="24" />
                      GitHub
                    </a>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      ) : null}

      {mode === "solver" && setupModal ? (
        setupModal.kind === "first" ? (
          <InstanceSetupModal
            title="Set up your default instance"
            subtitle="Pick the moves you'll actually use. This instance's base pruning table is built only from these moves, and only they are exposed in the move panel."
            initialName={setupModal.instance.name}
            initialMoves={setupModal.instance.moves}
            confirmLabel="Save"
            note={moveChooserNote}
            cancellable={false}
            onSave={(name, selected) => {
              const savedInstance: Instance = {
                id: setupModal.instance.id,
                name: name.trim() || "Default",
                moves: selected,
                banned: [],
              };
              setInstances((list) => {
                const exists = list.some((instance) => instance.id === savedInstance.id);
                return exists
                  ? list.map((instance) => (instance.id === savedInstance.id ? savedInstance : instance))
                  : [savedInstance];
              });
              setActiveId(savedInstance.id);
              setSetupModal(null);
            }}
            onCancel={() => setSetupModal(null)}
          />
        ) : (
          <InstanceSetupModal
            title="New instance"
            subtitle="Pick a name and the moves this instance will use. Its base pruning table is built only from these moves."
            initialName=""
            initialMoves={[...defaultInstanceMoves]}
            confirmLabel="Create"
            note={moveChooserNote}
            onSave={(name, selected) => {
              createInstance(name, selected);
              setSetupModal(null);
            }}
            onCancel={() => setSetupModal(null)}
          />
        )
      ) : null}

      <div hidden={mode !== "solver"}>
        <section className="workspace">
        <div className="input-pane">
          <div className="setup-row">
            <label className="field">
              <span className="input-mode-toggle">
                <button
                  type="button"
                  className={inputMode === "setup" ? "active" : ""}
                  onClick={() => setInputMode("setup")}
                >
                  Setup Move
                </button>
                <span>/</span>
                <button
                  type="button"
                  className={inputMode === "alg" ? "active" : ""}
                  onClick={() => setInputMode("alg")}
                >
                  alg
                </button>
              </span>
              <input
                value={setup}
                onChange={(event) => setSetup(event.target.value)}
                spellCheck={false}
                autoComplete="off"
              />
            </label>
            <button className="secondary" onClick={() => setApplySignal((current) => current + 1)}>Apply</button>
          </div>

          <FtoViewer
            viewerApiRef={viewerApiRef}
            active={mode === "solver" && !stateErrorsOpen}
            setup={setup}
            inputMode={inputMode}
            applySignal={applySignal}
            lastLayerMode={lastLayerMode}
            onFacelets={handleFacelets}
            onCenterTargets={setCenterTargets}
            faceColors={faceColors}
            stateText={stateErrors.length ? "invalid state" : "valid state"}
            stateInvalid={stateErrors.length > 0}
            onInvalidStateClick={() => setStateErrorsOpen(true)}
            footer={
              <div className="solver-output">
                <div className="output-toolbar">
                  <input
                    className="output-rotation"
                    aria-label="Execution rotation: original faces at U and F"
                    aria-invalid={!parseExecutionHold(rotationInput)}
                    title={parseExecutionHold(rotationInput)
                      ? `Execution hold (${executionHold.notation}): original faces at U,F. U,F is unchanged; R,F uses EIF.`
                      : "Use two distinct, non-opposite faces, such as U,F or R,F. The last valid hold is still shown."}
                    value={rotationInput}
                    placeholder="U,F"
                    spellCheck={false}
                    autoComplete="off"
                    autoCapitalize="characters"
                    onChange={(event) => {
                      const input = event.target.value;
                      setRotationInput(input);
                      const hold = parseExecutionHold(input);
                      if (hold) setExecutionHold(hold);
                    }}
                    onBlur={() => setRotationInput(executionHold.input)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" || event.key === "Escape") {
                        setRotationInput(executionHold.input);
                        event.currentTarget.blur();
                      }
                    }}
                  />
                </div>
                <div className="terminal" ref={terminalRef}>
                  {displayTerminal.length === 0 ? (
                    <div className="terminal-placeholder">solutions and solver logs will appear here</div>
                  ) : (
                    displayTerminal.map((line) => (
                      <div key={line.id} className={`terminal-line terminal-${line.kind}`}>
                        {line.kind === "solution" ? rotateAlgorithm(line.text, executionHold) : line.text}
                        {line.kind === "progress" ? (
                          <div className="terminal-progress-track">
                            <div
                              className="terminal-progress-fill"
                              style={{ width: `${pruningProgressPercent(line.text)}%` }}
                            />
                          </div>
                        ) : null}
                      </div>
                    ))
                  )}
                </div>
              </div>
            }
          />
        </div>

        <div className="control-pane">
          <section className="tool-section">
            <div className="section-heading">
              <div className="instance-menu panel-instance-menu">
                <button className="instance-trigger" onClick={() => setMenuOpen((open) => !open)}>
                  <span className="instance-trigger-name">{activeInstance.name}</span>
                  <span className="instance-caret">▾</span>
                </button>
                {menuOpen ? (
                  <>
                    <div className="menu-backdrop" onClick={closeOverlays} />
                    <div className="instance-menu-panel">
                      <div className="instance-menu-label">Instances</div>
                      {instances.map((instance) => (
                        <button
                          key={instance.id}
                          className={instance.id === activeId ? "instance-item active" : "instance-item"}
                          onClick={() => {
                            setActiveId(instance.id);
                            setMenuOpen(false);
                            setContextMenu(null);
                          }}
                          onContextMenu={(event) => openContextMenu(instance.id, event)}
                          title="Right-click to delete"
                        >
                          <span className="instance-item-name">{instance.name}</span>
                          <span className="instance-item-count">{instance.moves.length} moves</span>
                        </button>
                      ))}
                      <div className="instance-menu-divider" />
                      <button
                        className="instance-item new"
                        onClick={() => {
                          setSetupModal({ kind: "new" });
                          setMenuOpen(false);
                        }}
                      >
                        + New instance
                      </button>
                    </div>
                  </>
                ) : null}
              </div>
              <div className="mini-actions">
                <span className="move-count">{allowedMoves.length}/{activeInstance.moves.length} moves</span>
                <button onClick={allowAllMoves}>All</button>
                <button onClick={invertMoveSelection}>Invert</button>
              </div>
            </div>
            {activeInstance.moves.length === 0 ? (
              <div className="state-error">This instance has no moves selected. Edit it from the instance menu.</div>
            ) : (
              <div className="move-grid">
                {activeInstance.moves.map((move) => (
                  <button
                    key={move}
                    className={bannedSet.has(move) ? "move-toggle banned" : "move-toggle"}
                    onClick={() => toggleMove(move)}
                  >
                    {move}
                  </button>
                ))}
              </div>
            )}
          </section>

          <LlSetupSection
            facelets={facelets}
            viewerApiRef={viewerApiRef}
            lastLayerMode={lastLayerMode}
          />

          <section className="tool-section solve-options">
            <div className="section-heading">
              <h2>Search</h2>
            </div>
            <div className="search-grid">
            <label>
              Depth
              <input
                value={depth}
                onChange={(event) => setDepth(event.target.value.replace(/\D/g, ""))}
                inputMode="numeric"
                placeholder="auto"
              />
            </label>
            <label>
              Threads
              <input value={threads} onChange={(event) => setThreads(event.target.value)} inputMode="numeric" />
            </label>
            </div>
            <div className="option-list">
            <label className="check">
              <input type="checkbox" checked={all} onChange={(event) => setAll(event.target.checked)} />
              All solutions
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={lastLayerMode}
                onChange={(event) => setLastLayerMode(event.target.checked)}
              />
              Last layer mode
            </label>
            </div>
            <div className="actions-row">
            <button
              className={running ? "primary stop" : "primary"}
              onClick={running ? stopSolve : solve}
              disabled={!running && (!cubieState || allowedMoves.length === 0)}
            >
              {running ? "Stop" : "Solve"}
            </button>
            </div>
            <button className="secondary ram-button" onClick={unloadPruningTable} disabled={running || !cacheStatus.hasPruning}>
              Unload pruning table from RAM
            </button>
          </section>
        </div>
        </section>
      </div>
      <div hidden={mode !== "combiner"}>
        <AlgCombiner active={mode === "combiner"} />
      </div>
    </main>
  );
}

export default App;
