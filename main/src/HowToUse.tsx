import { useEffect, useRef } from "react";
import InputKeyboard from "./InputKeyboard";

const topics = [
  ["search-guide", "How to search"],
  ["solver-settings", "Solver settings"],
  ["input", "Input & Output"],
  ["tables", "Solver Performance"],
  ["batch", "Batch solving"],
  ["combiner", "Alg combiner"],
  ["settings", "Settings & shortcuts"],
] as const;

export default function HowToUse({ onClose }: { onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const backdropDown = useRef(false);

  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);

  function jump(id: string) {
    const section = dialog.current?.querySelector<HTMLElement>(`#how-to-${id}`);
    section?.scrollIntoView({ block: "start" });
    section?.focus({ preventScroll: true });
  }

  return (
    <dialog ref={dialog} className="how-to-modal" aria-labelledby="how-to-title"
      onCancel={(event) => { event.preventDefault(); onClose(); }}
      onClose={(event) => { if (!event.currentTarget.open) onClose(); }}
      onKeyDown={(event) => event.stopPropagation()}
      onPointerDown={(event) => { backdropDown.current = event.target === event.currentTarget; }}
      onClick={(event) => { if (backdropDown.current && event.target === event.currentTarget) onClose(); backdropDown.current = false; }}>
      <header className="how-to-header">
        <div className="modal-title-row">
          <h2 id="how-to-title">How to use</h2>
          <button type="button" className="secondary" autoFocus onClick={onClose}>Close</button>
        </div>
        <p>Why the app works this way, and how to get the most out of it.</p>
      </header>
      <div className="how-to-body">
        <nav aria-label="Help topics" className="how-to-topics">
          <span className="how-to-topics-label">In this guide</span>
          {topics.map(([id, label]) => <button key={id} type="button" onClick={() => jump(id)}>{label}</button>)}
        </nav>
        <label className="how-to-topic-picker">
          Jump to
          <select defaultValue="search-guide" onChange={(event) => jump(event.target.value)}>
            {topics.map(([id, label]) => <option key={id} value={id}>{label}</option>)}
          </select>
        </label>
        <div className="how-to-content">
        <section id="how-to-search-guide" tabIndex={-1}>
          <h3>How to search</h3>
          <h4>Why instances exist</h4>
          <p>Allowing every move when generating a pruning table makes the table take much longer to build and weakens its pruning, slowing down your searches too. An <strong>instance</strong> gives a particular move set its own space, with pruning tables built for that set.</p>
          <p>Instances serve three purposes:</p>
          <ol>
            <li><strong>Faster searches.</strong> Build tables around the moves you actually intend to use.</li>
            <li><strong>A cleaner right sidebar.</strong> Keep the relevant moves easy to find.</li>
            <li><strong>Separate workflows.</strong> Give each use of the solver its own move set and tables.</li>
          </ol>
          <p>Use the instance dropdown to switch or create instances. Expand <strong>More moves</strong> when creating one to include less-used moves. Right-click an instance name in the dropdown to delete it.</p>
          <p>The allowed-move buttons select which of the instance’s moves to search with. <strong>All</strong> and <strong>None</strong> toggle the whole set; <strong>Invert</strong> reverses the selection. To use a move outside the instance’s set, create an instance that includes it. Restricting moves can require longer algorithms or make a case unreachable.</p>
          <h4>Choose your generators carefully</h4>
          <p>This app is designed to find algorithms quickly. Treating it as a general-purpose FTO solver can lead to very long searches. The moves you allow have a huge effect on how much work the solver has to do.</p>
          <p>For a practical algorithm, you usually want to stick to four or five move types rather than mixing seven different ones. Start with move sets you would actually be comfortable executing.</p>
          <p>For example, suppose you like algorithms using any of these sets:</p>
          <ul>
            <li><code>R Rw U</code></li>
            <li><code>R Rw BR U</code></li>
            <li><code>R Rw F U</code></li>
            <li><code>R Rw U D</code></li>
            <li><code>R Rw U L</code></li>
          </ul>
          <p>It is tempting to enable <code>R Rw L U BR F D</code> and search them all at once. That also allows every mixture of those seven move types, making the search space much larger and potentially taking ages to finish.</p>
          <p><strong>Run those searches separately instead.</strong> Keep each search focused on one move set to get the most speed out of the solver and find algorithms you would actually use.</p>
          <p>You can speed up searches further with pruning and Bloom tables. See <a href="#how-to-tables" onClick={(event) => { event.preventDefault(); jump("tables"); }}>Pruning tables &amp; storage</a> for how to build and use them.</p>
        </section>

        <section id="how-to-solver-settings" tabIndex={-1}>
          <h3>Solver modes and settings</h3>
          <h4>Depth</h4>
          <p>Depth is the number of moves in the algorithm being searched for. A move and its inverse each count as one move. The depth field accepts a single number such as <code>12</code>, an inclusive range such as <code>12-15</code>, or a comma-separated mix such as <code>12-15, 17, 20</code>. Explicit depths are searched in ascending order; valid values are whole numbers from 0 to 255.</p>
          <p>The default, <strong>auto</strong>, searches progressively deeper until it finds a solution. Leaving the field blank also means auto. Use this when you want the shortest solution with your selected moves; enter specific depths when you want to explore particular algorithm lengths.</p>
          <p>The information button beside Depth shows the accepted input format. <strong>Stop</strong> ends a solver search; batch solving has its own Pause and resume controls.</p>
          <h4>All solutions</h4>
          <p>With <strong>All solutions</strong> off, the search stops after finding one solution. Turn it on to collect alternatives so you can compare algorithms and choose one that feels good to execute.</p>
          <p>With auto depth, All solutions collects solutions at the first successful depth. With explicit depths, it searches every selected depth and collects solutions at each one. Finding all solutions can take much longer than finding just one.</p>
          <h4>Last-layer mode</h4>
          <p>This mode is suitable for algsets that allow <strong>pre-AUF and post-AUF</strong>, such as Nautilus last layer. These are U adjustments before and after the algorithm. They are free in this mode, even when U is not an allowed search move: depth counts only the moves between them.</p>
          <p>Some center triangles share a color with the last-layer centers but belong outside the last layer. They appear in a slightly different shade to distinguish them from their same-color partners and indicate that they must finish outside the last layer. See <a href="#how-to-last-layer-input" onClick={(event) => { event.preventDefault(); jump("last-layer-input"); }}>last-layer input</a> for how to assign these pieces and set up a case.</p>
          <p>Last-layer mode has its own dedicated pruning tables. Ordinary pruning tables do not capture this goal well enough, so use tables built for Last-layer mode to get effective pruning. See <a href="#how-to-tables" onClick={(event) => { event.preventDefault(); jump("tables"); }}>Pruning tables &amp; storage</a> for table setup.</p>
          <h4>Threads</h4>
          <p>Multithreading splits the search across several workers so your CPU can explore different branches at the same time. Set Threads to <code>1</code> for a single worker, or increase it to use more of your CPU.</p>
          <p>A CPU with multiple cores can run workers in parallel. CPUs with simultaneous multithreading (SMT, also called Hyper-Threading on Intel CPUs) can run more than one hardware thread per core. You do not need a particular CPU brand; a useful starting point is the number of logical processors your system reports.</p>
          <p>The single-case solver has no fixed thread-count limit, so you can request as many workers as your hardware can support. Actual worker use depends on the available search branches. More threads than your CPU can run at once usually add overhead rather than speed, and leaving some capacity free can keep the rest of your computer responsive. Batch solving currently accepts 1–1,024 threads shared across its cases.</p>
        </section>

        <section id="how-to-input" tabIndex={-1}>
          <h3>Input</h3>
          <p>There are four ways to enter a state: edit the graphical FTO, apply an algorithm or setup, use the keyboard, or choose cases from the last-layer table. You can combine these methods to build the case you want.</p>
          <ol className="how-to-input-methods">
            <li><a href="#how-to-viewer" onClick={(event) => { event.preventDefault(); jump("viewer"); }}>Manipulating the graphical FTO</a></li>
            <li><a href="#how-to-alg-input" onClick={(event) => { event.preventDefault(); jump("alg-input"); }}>Applying algorithms and setup moves</a></li>
            <li><a href="#how-to-keyboard-input" onClick={(event) => { event.preventDefault(); jump("keyboard-input"); }}>Keyboard input</a></li>
            <li><a href="#how-to-last-layer-input" onClick={(event) => { event.preventDefault(); jump("last-layer-input"); }}>Last-layer table input</a></li>
          </ol>

          <h4 id="how-to-viewer" tabIndex={-1}>1. Manipulating the graphical FTO</h4>
          <ul>
            <li><strong>Paint:</strong> choose a color swatch to enter Paint mode, then click a sticker to color it. This colors individual stickers; it does not automatically recolor the rest of a corner or edge. To orbit without painting, start your drag on empty space.</li>
            <li><strong>Swap:</strong> click a piece, then another piece of the same kind to exchange them. The first piece has a pink outline. Clicking a different kind selects that piece instead. Click the same corner twice to twist it; click the same edge or center twice, or click empty space, to clear the selection.</li>
            <li><strong>View:</strong> restores the default camera angle.</li>
            <li><strong>Reset:</strong> restores the solved puzzle and clears manually selected center target slots.</li>
          </ul>
          <p>Painting and swapping can create an impossible state. If the viewer marks the state invalid, click that label to see all detected problems and which pieces or color counts need checking.</p>

          <div id="how-to-partial" tabIndex={-1}>
          <h5>Partial positions &amp; center slots</h5>
          <p><strong>Right-click a piece</strong> to ignore it. The solver then solves only the defined pieces optimally not caring about ignored pieces. Right-click it again to define it and restore its previous colors.</p>
          <p><strong>Entitling a slot means selecting it as a destination for your defined triangle of that color.</strong> A triangle piece has 3 different slots to be in. When only one or two triangles are defined and you want them to end up in a specific slot (such as Nautilus LT) you can use this feature. Shift-click a center slot to give it a golden outline; Shift-click again to remove it. It indicates if there are enough defined triangle of that color the solver will prioritize putting those triangles in the gold-outlined slots.</p>

          <h5>Manually assigning last-layer centers</h5>
          <p>With Last-layer mode enabled, the bottom triangles of the last layer colors are shaded differently and it makes sure in the solved state the triangles don't end up in the Last Layer. You can swap the last layer bottom triangle around, or in Paint mode, select the color and <strong>Ctrl+Left-click a triangle</strong> to assign the distinguished piece that must finish outside the last layer.</p>
          </div>

          <h4 id="how-to-alg-input" tabIndex={-1}>2. Applying algorithms and setup moves</h4>
          <p>Enter your sequence in the input box, choose <strong>alg</strong> or <strong>Setup Move</strong>, then click <strong>Apply</strong>. Apply acts on the current state of the graphical FTO. Hold <strong>Shift</strong> while clicking Apply to apply the moves on a solved puzzle.</p>
          <p>For move notation, refer to the <a href="https://docs.google.com/document/d/1W0nqrep_Xm0qe4GHxrq61czHmGPy1DDXEACIliXiHnM/edit?tab=t.0#heading=h.46zs529hnp9y" target="_blank" rel="noreferrer">FTO notation guide</a>.</p>
          <p>Wide-move input accepts both lowercase letters and a <code>w</code> suffix, such as <code>r</code> or <code>Rw</code>, and <code>br</code> or <code>BRw</code>. Slice input accepts both <code>M N E S</code> and the <code>s</code>-suffixed notation <code>Rs Ls Us Fs</code>. Output uses the standard lowercase wide moves and <code>s</code>-suffixed slice notation.</p>
          <p><strong>Input orientation:</strong> insert an orientation marker like <code>{"{U,R}"}</code> to indicate which orientation you want the moves to be applied in. A marker affects only the following moves. Each input starts at <code>{"{U,F}"}</code>.</p>

          <p><strong>Output orientation:</strong> the execution-angle field changes how solutions and move labels are written for your hold. For example, to find an R, r, and U gen EIF algorithm, select BR, br, and U, then set the execution angle to U,R. The output will be translated into the EIF R, r, and U notation</p>

          <h4 id="how-to-keyboard-input" tabIndex={-1}>3. Keyboard input</h4>
          <p>You can use your keyboard to turn the puzzle like a virtual FTO. The keybinds are the same as csTimer. Reference keybind layout:</p>
          <InputKeyboard />

          <h4 id="how-to-last-layer-input" tabIndex={-1}>4. Last-layer table input</h4>
          <p>Click <strong>Choose cases</strong> beside <strong>Set up LL</strong> in the sidebar. The floating picker lets you combine cases from the L6X and L3C tabs, with U adjustments using the fixed U and U' buttons. The case grid scrolls independently, and the picker stays open while you build the case.</p>
          <ol>
            <li><strong>Choose an L6X case.</strong></li>
            <li><strong>Adjust the alignment.</strong></li>
            <li><strong>Choose a L3C case.</strong> You can choose L3C before L6X too.</li>
          </ol>
          <p>Click another option in the same group to replace its pattern. Click the selected option again to clear that group back to solved relative to the current AUF. The other group is kept when compatible. The thumbnails update as you build the case.</p>
        </section>

        <section id="how-to-tables" tabIndex={-1}>
          <h3>Pruning tables &amp; storage</h3>
          <p>Open <strong>⋮ → Pruning</strong> to generate pattern or Bloom tables. Match the table’s move set and goal mode to the searches you plan to run. Compatible saved tables are reused automatically; a table can cover a search using a subset of its moves.</p>
          <p>Bloom tables store a neighborhood around the goal. Choose a depth and memory budget, and select Last-layer mode when that is the intended goal.</p>
          <p>Both Pruning tabs show the full move pool. Opening Pruning copies the moves currently selected in the solver; you can then choose a different set for the table. Labels follow the execution hold.</p>
          <p><strong>Undefine pieces…</strong> opens a separate solved puzzle. Click whole pieces to ignore or restore them, and drag to orbit. Use these pieces saves the table goal without changing the main puzzle. Pattern tables use a joint coordinate projection with a default budget of 512 MiB. The actual size depends on the defined pieces and useful projections; a larger table does not guarantee a particular search speedup.</p>
          <p>Partial Bloom tables store a near-goal membership filter for the selected partial goal. Their memory cap controls the saved filter, while the projection workspace budget controls the temporary coordinate table. Compatible partial searches, including batch cases, load dedicated tables automatically and retain the ordinary piece heuristics. A search may use a subset of the table’s moves and may define additional pieces. Last-layer mode must match.</p>
          <p><strong>CPU is the default for table generation.</strong> Enable GPU acceleration in Settings to remember it for later builds, or click the underlined Enable GPU acceleration link at the bottom right of the output area during a CPU build. The link affects the current build; Settings controls the default for later builds. The app checks the request within the current depth and runs a short comparison of newly visited nodes per second. It tells you if the GPU is slower and continues on CPU with completed batches retained. Progress updates during a depth. Stop cancels transition and pruning generation, including table loading and saving. Reloading the interface reconnects to an active operation so it can still be stopped.</p>
          <p><strong>Settings → Manage storage</strong> lists saved tables and lets you delete them. Unload pruning table from RAM releases the loaded tables while keeping their saved files. Deleting a saved table means it may need to be generated again for a later search.</p>
        </section>

        <section id="how-to-batch" tabIndex={-1}>
          <h3>Batch solving</h3>
          <p>Choose <strong>⋮ → Switch to batch solving</strong>. Enter one setup per numbered line. Pasting multiple lines creates multiple cases. Each case starts with its own default input orientation.</p>
          <ul>
            <li>Press <strong>Tab</strong> on a setup line to open its visual editor. Saved visual cases appear as <strong>Case N</strong> links. Click a link to reopen it.</li>
            <li>Select across lines and use Backspace or Delete to remove cases. Undo and redo restore text edits, including visual entries.</li>
            <li>The sidebar applies the selected moves, depths, threads, and goal mode to the batch. <strong>Use current instance</strong> copies the solver’s current move choices.</li>
            <li><strong>Pause</strong> preserves completed depths, found solutions, and unfinished search branches. Resume continues that work. Unchanged cases keep their progress when other cases are edited.</li>
          </ul>
          <p><strong>Defer after (seconds)</strong> applies in All solutions mode. A slow first-pass case can be held at its current depth while other cases catch up, then resumed without that time limit. Set 0 to disable deferral. This changes search order; it does not set a total time limit for a case.</p>
          <p>Projects autosave locally. Download project creates a portable copy with progress; Open project restores it. Export CSV writes solution rows and includes unsolved or invalid cases. Changing search settings resets progress, except for changes to threads or the deferral budget.</p>
        </section>

        <section id="how-to-combiner" tabIndex={-1}>
          <h3>Alg combiner</h3>
          <p>Choose <strong>⋮ → Switch to alg combiner</strong>. Set up the target case in its viewer, then enter intermediate algorithms one per line, optionally named with a colon: <code>trigger: R U R'</code>.</p>
          <p>The combiner tries individual algorithms and ordered pairs with U adjustments around and between them. Enable three-alg combinations to include ordered triples. This increases the number of candidates substantially.</p>
          <p>Results include the contributing algorithms and the move count after adjacent moves cancel. The combiner searches combinations from your list; it does not run the solver’s unrestricted move search.</p>
        </section>

        <section id="how-to-settings" tabIndex={-1}>
          <h3>Settings &amp; shortcuts</h3>
          <p><strong>Settings → Face colors</strong> changes the display palette; puzzle identities and notation stay the same. Show debug info exposes more search output. Reset all app data is separate from the viewer’s Reset button.</p>
          <p><strong>Ctrl+= / Ctrl+-</strong> changes app zoom; <strong>Ctrl+0</strong> restores 100%. The zoom level is remembered. On platforms using Command shortcuts, Command works too.</p>
          <p>Press Escape or Close to dismiss this guide.</p>
        </section>
        </div>
      </div>
    </dialog>
  );
}
