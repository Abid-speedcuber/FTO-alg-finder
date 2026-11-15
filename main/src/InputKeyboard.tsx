type Key = { key: string; action?: string; kind?: "wide" | "rotation"; width?: number };
const bindings: Record<string, string> = {
  "3": "BL'", "8": "BR", W: "B", E: "L'", R: "Lw'", T: "Rotate L'", Y: "Rotate R",
  U: "Rw", I: "R", O: "B'", A: "Rotate U'", S: "D", D: "L", F: "U'", G: "F'",
  H: "F", J: "U", K: "R'", L: "D'", ";": "Rotate U", C: "BL", V: "Lw",
  B: "Rotate L", N: "Rotate R'", M: "Rw'", ",": "BR'",
};
const key = (label: string, width = 1): Key => ({
  key: label, action: bindings[label], width,
  kind: bindings[label]?.startsWith("Rotate") ? "rotation" : bindings[label]?.includes("w") ? "wide" : undefined,
});
const rows: Key[][] = [
  ["`", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "-", "="].map(label => key(label)).concat(key("Backspace", 2)),
  [key("Tab", 1.5), ...["Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P", "[", "]"].map(label => key(label)), key("\\", 1.5)],
  [key("Caps", 1.75), ...["A", "S", "D", "F", "G", "H", "J", "K", "L", ";", "'"].map(label => key(label)), key("Enter", 2.25)],
  [key("Shift", 2.25), ...["Z", "X", "C", "V", "B", "N", "M", ",", ".", "/"].map(label => key(label)), key("Shift", 2.75)],
  [key("Ctrl", 1.5), key("Meta", 1.25), key("Alt", 1.25), key("Space", 7), key("Alt", 1.25), key("Meta", 1.25), key("Ctrl", 1.5)],
];

export default function InputKeyboard() {
  return <figure className="how-to-keyboard">
    <div className="how-to-keyboard-scroll" role="img" aria-label="Keyboard shortcuts in QWERTY positions">
      <div className="how-to-keyboard-layout">
        {rows.map((row, index) => <div className="how-to-keyboard-row" key={index}>
          {row.map((item, column) => <div key={column}
            className={`how-to-key ${item.action ? "is-bound" : ""} ${item.kind ?? ""}`}
            style={{ flex: `${item.width} 0 0` }}
            aria-label={`${item.key}: ${item.action ?? "no viewer shortcut"}`}>
            {item.action && <span className="how-to-key-action">{item.kind === "rotation"
              ? <>Rot<br />{item.action.replace("Rotate ", "")}</>
              : item.action}</span>}
          </div>)}
        </div>)}
      </div>
    </div>
  </figure>;
}
