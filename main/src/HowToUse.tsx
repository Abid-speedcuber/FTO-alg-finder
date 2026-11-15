import { useEffect, useRef } from "react";

const topics = [
  ["search-guide", "How to search"],
  ["input", "Input & notation"],
  ["viewer", "Viewer controls"],
  ["partial", "Partial positions"],
  ["last-layer", "Last-layer mode"],
  ["search", "Moves & search"],
  ["tables", "Tables & storage"],
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
          <p><strong>Run those four searches separately instead.</strong> Keep each search focused on one move set to get the most speed out of the solver and find algorithms you would actually use.</p>
          <p>You can speed up searches further with pruning and Bloom tables. See <a href="#how-to-tables" onClick={(event) => { event.preventDefault(); jump("tables"); }}>Pruning tables &amp; storage</a> for how to build and use them.</p>
        </section>

        <section id="how-to-input" tabIndex={-1}>
          <h3>Input &amp; notation</h3>
          <p><strong>Setup Move</strong> applies the sequence you enter. <strong>alg</strong> applies its inverse to construct a case for that algorithm. Apply acts on the current puzzle. Hold Shift while clicking Apply to reset to solved first, then apply the input. This works in Setup Move and alg mode, and in the combiner; Shift+Enter also works in the combiner input. The batch visual editor’s Apply setup always starts from solved.</p>
          <p>Separate moves with spaces. A prime means the inverse turn; <code>R2</code> also means <code>R'</code> on an FTO. Parentheses can group moves. Both lowercase wide moves and the older w spelling work: <code>r = Rw</code>, <code>br = BRw</code>, and likewise for the other faces.</p>
          <div className="how-to-table-scroll"><table>
            <caption>Slice directions and aliases</caption>
            <thead><tr><th>Slice</th><th>Follows</th><th>Expansion</th><th>Old spelling</th></tr></thead>
            <tbody>
              <tr><td><code>Rs</code></td><td>R layer</td><td><code>r R'</code></td><td><code>M'</code></td></tr>
              <tr><td><code>Ls</code></td><td>L layer</td><td><code>l L'</code></td><td><code>N</code></td></tr>
              <tr><td><code>Us</code></td><td>U layer</td><td><code>u U'</code></td><td><code>E'</code></td></tr>
              <tr><td><code>Fs</code></td><td>F layer</td><td><code>f F'</code></td><td><code>S</code></td></tr>
            </tbody>
          </table></div>
          <p><code>M = Rs'</code> and <code>E = Us'</code>. The aliases also accept inverses, such as <code>N' = Ls'</code>.</p>
          <p><strong>Input orientation:</strong> insert <code>{"{U,R}"}</code> to say which original faces you are holding at U and F. A marker affects only the following moves, until another marker. Each input starts at <code>{"{U,F}"}</code>; that marker also resets the hold mid-sequence. Use distinct, non-opposite faces.</p>
          <ul>
            <li><code>{"{U,R} R U R' U'"}</code> means <code>BR U BR' U'</code>.</li>
            <li><code>{"R' {U,L} R U R' U'"}</code> means <code>R' F U F' U'</code>.</li>
          </ul>
          <p>These markers work in batch setups and the combiner too. They are resolved before the sequence is inverted in alg mode.</p>
          <p><strong>Output orientation:</strong> the execution-angle field changes how solutions and move labels are written for your hold. It does not change the input state or the physical moves selected. Input markers translate into the original frame; the output hold translates from that frame into your execution hold.</p>
        </section>

        <section id="how-to-viewer" tabIndex={-1}>
          <h3>Viewer controls</h3>
          <ul>
            <li><strong>Pan:</strong> drag to rotate the view. While dragging in Pan, the mouse wheel rolls the view.</li>
            <li><strong>Paint:</strong> choose a color swatch, then click a sticker. Choosing a swatch enters paint mode.</li>
            <li><strong>Swap:</strong> click a piece, then another piece of the same kind. The selected piece has a pink outline. Click the same corner twice to twist it. Click empty space to clear the selection.</li>
            <li><strong>View:</strong> restores the camera angle.</li>
            <li><strong>Reset:</strong> restores the solved puzzle and clears manually selected center target slots.</li>
          </ul>
          <p>If the state is marked invalid, click the invalid-state label to see the detected problems. Painting or swapping can create an impossible state; the list identifies the pieces or color counts to check.</p>
        </section>

        <section id="how-to-partial" tabIndex={-1}>
          <h3>Partial positions &amp; center slots</h3>
          <p><strong>Right-click a piece</strong> to ignore it. All stickers of that piece become black. The solver then solves the defined pieces and lets ignored pieces finish elsewhere. Right-click it again to define it and restore its previous colors. This works for corners, edges, and individual center triangles.</p>
          <p>The remembered colors follow the piece through turns and swaps. A piece loaded already black has no remembered colors, so right-click defines it using its home-slot colors; you can repaint those if needed.</p>
          <p><strong>A piece moves; a slot is a place it can finish.</strong> Each center color has three home slots in the solved puzzle. A red home slot is still a red destination even if a blue sticker currently occupies it. A defined center is one you have colored in; a black, ignored center is not defined.</p>
          <p><strong>Entitling a slot means selecting it as a destination for your defined centers of that color.</strong> Shift-click a center slot to give it a golden outline; Shift-click again to remove it. This works in Pan, Paint, and Swap. It changes the solve’s destination constraint without painting the sticker, and the selected paint color has no effect. The outline stays with the slot when pieces move.</p>
          <p><strong>Example:</strong> you have defined one red center and ignored the other two. Normally, that piece may finish in any of the three red home slots. Give one red slot a golden outline to make it finish there. Give two red slots golden outlines to let the solver choose either one. You are choosing where the red piece may go, not choosing or recoloring the piece currently in that slot.</p>
          <p>With two defined red centers, selecting one slot means at least one of those two pieces must reach it. Selecting two slots means both defined pieces must fill those two slots, in either order. The solver matches as many defined centers to selected slots as possible; it does not distinguish same-color pieces.</p>
          <div className="how-to-table-scroll"><table>
            <caption>Red-center examples</caption>
            <thead><tr><th>Defined red centers</th><th>Gold-outlined red home slots</th><th>Required result</th></tr></thead>
            <tbody>
              <tr><td>1</td><td>1</td><td>That piece must reach that slot.</td></tr>
              <tr><td>1</td><td>2</td><td>That piece must reach either selected slot.</td></tr>
              <tr><td>2</td><td>1</td><td>Either defined piece must reach that slot.</td></tr>
              <tr><td>2</td><td>2</td><td>Both defined pieces must fill the selected slots, in either order.</td></tr>
            </tbody>
          </table></div>
          <p><strong>When the outlines add no restriction:</strong> zero or all three centers of that color are defined, or zero or all three home slots are selected. Defined centers still finish on their own color as usual. For example, selecting all three red slots gives a defined red piece the same three destinations it already had.</p>
        </section>

        <section id="how-to-last-layer" tabIndex={-1}>
          <h3>Last-layer mode</h3>
          <p>Last-layer mode distinguishes the odd center of each affected color from its two interchangeable partners. Only the odd triangle is shaded: light colors become darker, and dark colors become lighter.</p>
          <p>To assign that odd piece manually, enter Paint, choose its color, then <strong>Ctrl-click the center</strong>. A normal paint click defines one of the interchangeable partners. This piece assignment is separate from the golden target-slot selection.</p>
          <p>Leading and trailing U adjustments are free in this mode, including when U is not an allowed search move. Depth counts the moves between those adjustments. The goal keeps the distinguished centers in their required slots.</p>
          <p><strong>Set up LL</strong> expands into EP, AUF, and CP choices. The thumbnails preview each change. If a choice requires resetting another part of the case, the app asks before applying it. The U and U' buttons adjust the current setup.</p>
        </section>

        <section id="how-to-search" tabIndex={-1}>
          <h3>Moves, instances &amp; search</h3>
          <p>An <strong>instance</strong> stores a move set and uses pruning tables built for it. Open the instance dropdown to switch or create instances; expand More moves when creating one to include less-used moves. Right-click an instance name in the dropdown to delete it.</p>
          <p>The allowed-move buttons enable or disable moves within that instance. All and None toggle the whole set; Invert reverses which moves are enabled. If a move is outside the instance’s set, create an instance that includes it. A smaller move set can make pruning more effective, but it can also require longer solutions or make a case unreachable.</p>
          <ul>
            <li><strong>Depth blank or auto:</strong> search until the first successful depth.</li>
            <li><strong>Explicit depths:</strong> <code>12-15, 17, 20</code> searches those exact depths in ascending order. Ranges are inclusive; valid depths are 0–255.</li>
            <li><strong>All solutions:</strong> with explicit depths, collect solutions at every selected depth. With auto, collect solutions at the first successful depth. With All disabled, stop after finding one solution.</li>
            <li><strong>Threads:</strong> sets the requested parallel search workers.</li>
          </ul>
          <p>The small information button beside Depth explains the depth syntax. Stop ends a solver search; batch mode has a separate Pause and resume workflow.</p>
        </section>

        <section id="how-to-tables" tabIndex={-1}>
          <h3>Pruning tables &amp; storage</h3>
          <p>Open <strong>⋮ → Pruning</strong> to generate pattern or Bloom tables. Match the table’s move set and goal mode to the searches you plan to run. Compatible saved tables are reused automatically; a table can cover a search using a subset of its moves.</p>
          <p>Bloom tables store a neighborhood around the goal. Choose a depth and memory budget, and select Last-layer mode when that is the intended goal. Full-position and last-layer tables serve different goals. Partial-position searches use their partial pruning.</p>
          <p>Both Pruning tabs show the full move pool. Opening Pruning copies the moves currently selected in the solver; you can then choose a different set for the table. Labels follow the execution hold.</p>
          <p><strong>Undefine pieces…</strong> opens a separate solved puzzle. Click whole pieces to ignore or restore them, and drag to orbit. Use these pieces saves the table goal without changing the main puzzle. Pattern tables use a joint coordinate projection with a default budget of 512 MiB. The actual size depends on the defined pieces and useful projections; a larger table does not guarantee a particular search speedup.</p>
          <p>Partial Bloom tables store a near-goal membership filter for the selected partial goal. Their memory cap controls the saved filter, while the projection workspace budget controls the temporary coordinate table. Compatible partial searches, including batch cases, load dedicated tables automatically and retain the ordinary piece heuristics. A search may use a subset of the table’s moves and may define additional pieces. Last-layer mode must match.</p>
          <p><strong>CPU is the default for table generation.</strong> Enable GPU acceleration in Settings to remember it for later builds, or click the underlined Enable GPU acceleration link at the bottom right of the output area during a CPU build. The link affects the current build; Settings controls the default for later builds. The app checks the request within the current depth and runs a short comparison of newly visited nodes per second. It tells you if the GPU is slower and continues on CPU with completed batches retained. Progress updates during a depth. Stop cancels transition and pruning generation, including table loading and saving. Reloading the interface reconnects to an active operation so it can still be stopped.</p>
          <p><strong>Settings → Manage storage</strong> lists saved tables and lets you delete them. Unload pruning table from RAM releases the loaded tables while keeping their saved files. Deleting a saved table means it may need to be generated again for a later search.</p>
        </section>

        <section id="how-to-batch" tabIndex={-1}>
          <h3>Batch solving</h3>
          <p>Choose <strong>⋮ → Switch to batch solving</strong>. Enter one setup per numbered line. Pasting multiple lines creates multiple cases; all notation aliases and input orientation markers work here. Each case starts with its own default orientation.</p>
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
          <p>When you are not typing in a text field, the viewer accepts these basic turn keys:</p>
          <div className="how-to-table-scroll"><table>
            <caption>Viewer turn keys</caption>
            <thead><tr><th>Move</th><th>Key</th><th>Inverse key</th></tr></thead>
            <tbody>
              <tr><td>R</td><td><kbd>I</kbd></td><td><kbd>K</kbd></td></tr>
              <tr><td>L</td><td><kbd>D</kbd></td><td><kbd>E</kbd></td></tr>
              <tr><td>U</td><td><kbd>J</kbd></td><td><kbd>F</kbd></td></tr>
              <tr><td>F</td><td><kbd>H</kbd></td><td><kbd>G</kbd></td></tr>
              <tr><td>D</td><td><kbd>S</kbd></td><td><kbd>L</kbd></td></tr>
              <tr><td>B</td><td><kbd>W</kbd></td><td><kbd>O</kbd></td></tr>
            </tbody>
          </table></div>
          <p>Press Escape or Close to dismiss this guide.</p>
        </section>
        </div>
      </div>
    </dialog>
  );
}
