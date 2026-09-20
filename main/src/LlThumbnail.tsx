// Flattened top layer: the yellow face with its three side strips folded out.
// Cell positions are fixed slots; colors come from the facelet array.

import { EP_FACELET_SET } from "./llSetup";

type Point = [number, number];
type Cell = { facelet: number; points: string };

const H = Math.sqrt(3) / 2;
const CENTRE: Point = [1.5, H];

// Yellow facelets: corner 8 at TL, 4 at TR, 0 at the bottom tip.
const YELLOW_CELLS: Array<[number, Point[]]> = [
  [8, [[0, 0], [1, 0], [0.5, H]]],
  [7, [[1, 0], [1.5, H], [0.5, H]]],
  [6, [[1, 0], [2, 0], [1.5, H]]],
  [5, [[2, 0], [2.5, H], [1.5, H]]],
  [4, [[2, 0], [3, 0], [2.5, H]]],
  [3, [[0.5, H], [1.5, H], [1, 2 * H]]],
  [2, [[1.5, H], [2, 2 * H], [1, 2 * H]]],
  [1, [[1.5, H], [2.5, H], [2, 2 * H]]],
  [0, [[1, 2 * H], [2, 2 * H], [1.5, 3 * H]]],
];

// Strip cells run left to right: corner, center, edge, center, corner.
const STRIP_CELLS: Point[][] = [
  [[0, 0], [1, 0], [0.5, -H]],
  [[0.5, -H], [1.5, -H], [1, 0]],
  [[1, 0], [2, 0], [1.5, -H]],
  [[1.5, -H], [2.5, -H], [2, 0]],
  [[2, 0], [3, 0], [2.5, -H]],
];

// Rotating the top strip by +120 degrees lands on the right strip, -120 on the left.
const STRIPS: Array<{ rotation: number; facelets: number[] }> = [
  { rotation: 0, facelets: [49, 50, 51, 52, 53] },
  { rotation: 120, facelets: [62, 61, 57, 56, 54] },
  { rotation: -120, facelets: [63, 65, 64, 68, 67] },
];

function rotate(point: Point, degrees: number): Point {
  if (degrees === 0) {
    return point;
  }
  const radians = (degrees * Math.PI) / 180;
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);
  const dx = point[0] - CENTRE[0];
  const dy = point[1] - CENTRE[1];
  return [CENTRE[0] + dx * cos - dy * sin, CENTRE[1] + dx * sin + dy * cos];
}

function buildCells(): { cells: Cell[]; viewBox: string } {
  const raw: Array<{ facelet: number; points: Point[] }> = YELLOW_CELLS.map(
    ([facelet, points]) => ({ facelet, points }),
  );
  for (const strip of STRIPS) {
    strip.facelets.forEach((facelet, index) => {
      raw.push({
        facelet,
        points: STRIP_CELLS[index].map((point) => rotate(point, strip.rotation)),
      });
    });
  }

  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const cell of raw) {
    for (const [x, y] of cell.points) {
      minX = Math.min(minX, x);
      minY = Math.min(minY, y);
      maxX = Math.max(maxX, x);
      maxY = Math.max(maxY, y);
    }
  }
  const pad = 0.08;
  const viewBox = [
    minX - pad,
    minY - pad,
    maxX - minX + pad * 2,
    maxY - minY + pad * 2,
  ].join(" ");

  const cells = raw.map((cell) => ({
    facelet: cell.facelet,
    points: cell.points.map(([x, y]) => `${x.toFixed(4)},${y.toFixed(4)}`).join(" "),
  }));
  return { cells, viewBox };
}

const { cells: LL_CELLS, viewBox: LL_VIEW_BOX } = buildCells();

const COLOR_HEX = [
  "#ffff00",
  "#0000ff",
  "#ff0000",
  "#800080",
  "#ffffff",
  "#00a050",
  "#808080",
  "#ff8800",
  "#050505",
];

// Matches the viewer's lighter shading for last-layer center triangles.
const LAST_LAYER_HEX: Record<number, string> = {
  5: "#51e86f",
  6: "#b8b8b8",
  7: "#ffb34a",
};

function fillFor(color: number | undefined, facelet: number, lastLayerMode: boolean): string {
  if (color == null) {
    return "#2b3036";
  }
  if (lastLayerMode && EP_FACELET_SET.has(facelet) && LAST_LAYER_HEX[color]) {
    return LAST_LAYER_HEX[color];
  }
  return COLOR_HEX[color] ?? "#2b3036";
}

function LlThumbnail({
  facelets,
  lastLayerMode,
}: {
  facelets: number[];
  lastLayerMode: boolean;
}) {
  return (
    <svg className="ll-thumb" viewBox={LL_VIEW_BOX} role="img" aria-hidden="true">
      {LL_CELLS.map((cell) => (
        <polygon
          key={cell.facelet}
          points={cell.points}
          fill={fillFor(facelets[cell.facelet], cell.facelet, lastLayerMode)}
          stroke="#15181c"
          strokeWidth={0.07}
          strokeLinejoin="round"
        />
      ))}
    </svg>
  );
}

export default LlThumbnail;
