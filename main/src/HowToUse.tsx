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

export default function HowToUse({ onClose, initialTopic = "search-guide" }: { onClose: () => void; initialTopic?: string }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const backdropDown = useRef(false);

  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    if (initialTopic !== "search-guide") jump(initialTopic);
    return () => element?.close();
  }, [initialTopic]);

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
          <select defaultValue={initialTopic} onChange={(event) => jump(event.target.value)}>
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
          <p>You can speed up searches further with pruning and Bloom tables. See <a href="#how-to-tables" onClick={(event) => { event.preventDefault(); jump("tables"); }}>Solver performance</a> for how to build and use them.</p>
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
          <p>Last-layer mode has its own dedicated pruning tables. Ordinary pruning tables do not capture this goal well enough, so use tables built for Last-layer mode to get effective pruning. See <a href="#how-to-tables" onClick={(event) => { event.preventDefault(); jump("tables"); }}>Solver performance</a> for table setup.</p>
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
          <h3>Solver performance</h3>
          <h4>Why heuristic quality matters</h4>
          <p>The solver spends most of its time deciding which move sequences it can rule out. A <strong>heuristic</strong> gives a lower bound on the moves still needed. If the bound says a branch needs at least eight moves but the search has only six moves left in its budget, that branch can be skipped entirely. Better bounds mean less searching; extra threads alone cannot compensate for a weak bound.</p>
          <p>Pattern tables store distances for groups of pieces, so the solver can reason about their interactions instead of checking each piece separately. Bloom tables add a different test: whether a state could be within a stored distance of the goal. The solver uses these checks together. They reduce the work without changing which states count as solved or sacrificing optimality.</p>
          <p>The first way to improve table quality is to <strong>build for the moves and goal you actually use</strong>. A table built with extra moves assumes those extra shortcuts are available, so its estimates can be weaker for a restricted search. An exact move-set match is a useful starting point. See <a href="#how-to-search-guide" onClick={(event) => { event.preventDefault(); jump("search-guide"); }}>How to search</a> for choosing your generators.</p>

          <h4>Generating pattern tables</h4>
          <p>Open <strong>⋮ → Pruning → Pattern tables</strong>. Choose the moves to build for, set <strong>Use last layer mode</strong> to match your intended search, then click <strong>Generate pruning table</strong>.</p>

          <h4>Tables for partial positions</h4>
          <p>If you are generating an algset (for example, LT, LBT, or L2C) with the same pieces ignored in every case, dedicated partial-position pruning can help. The ordinary partial heuristics are prepared for each search and can be less effective on complex cases; a saved joint table can strengthen them across repeated searches. Click <strong>Undefine pieces</strong> in either Pruning tab. A graphical FTO opens where you can undefine pieces by left-clicking them, or click again to restore them. Click <strong>Use these pieces</strong> to save the table goal.</p>
          <p>On the Pattern tables tab, <strong>Pattern table budget (MiB)</strong> controls the size of the joint projection the app can choose. The default is 512 MiB; accepted values are 1–4,096 MiB. The app chooses useful piece combinations that fit, so the resulting table may be smaller than the budget. A bigger budget can allow a stronger projection, but a bigger file does not guarantee a faster search.</p>
          <p>Define the pieces your searches actually need. Extra constraints can strengthen a table, but a table that requires pieces your search ignores is not compatible. The solver can reuse a partial table when the search defines additional pieces and uses a subset of the table’s moves. Last-layer mode must also match. Compatible dedicated tables supplement the ordinary partial-piece heuristics, including in batch solving.</p>

          <h4>What Bloom tables add</h4>
          <p>Open <strong>⋮ → Pruning → Bloom tables</strong>. Choose the moves, goal mode, <strong>Depth</strong>, and <strong>Memory cap (MiB)</strong>, then click <strong>Generate Bloom table</strong>. A depth-six full-position Bloom table represents states within six moves of solved. Its depth is the size of that stored neighborhood, <strong>not the maximum depth of your algorithm search</strong>.</p>
          <p>A negative membership result lets the solver reject a branch that cannot finish within that neighborhood. A positive result only means “possibly”; the solver still searches and checks the actual goal. False positives cost time, not correctness. Bloom tables do not contain ready-made finishing algorithms, and their depth is not added to the pattern-table estimate.</p>
          <p>A deeper table can rule out branches earlier, but generation grows rapidly with both depth and the number of allowed moves. It helps most when your searches reach that remaining depth and ordinary pruning leaves work for it to eliminate. In my 20-case benchmark using U/F/R/BL, compared with pattern pruning alone, a depth-5 Bloom table reduced the node count by about 8% and CPU time by 7.3%. A depth-7 Bloom table reduced nodes by about 18% and CPU time by 30%, while a depth-8 table made the search about 2.7× faster by CPU time. These were full-position, single-threaded searches at exact depths 16 and 17; the benefit varies with the cases, moves, and existing pruning tables.</p>

          <h4>Choosing Bloom depth and memory</h4>
          <ol>
            <li><strong>Start with a focused move set.</strong> Build separate tables for the generator sets you actually search. Combining unrelated move types makes the neighborhood grow much faster and can greatly increase build time and memory requirements.</li>
            <li><strong>Start shallow and increase one level at a time.</strong> Depth five or six is a practical first trial for a small generator set. The UI accepts depths 1–255, but that does not mean every depth is practical for every move set. There is no universal depth that guarantees a build will fit.</li>
            <li><strong>Choose a cap that leaves RAM free.</strong> The default Bloom cap is 200 MiB. It limits the filter, not the entire app. Loaded pruning and transition tables, generation workspace, the interface, and other applications also need memory. Leave headroom rather than allocating all available RAM to the filter.</li>
            <li><strong>Distinguish a full filter from insufficient RAM.</strong> If the filter becomes too crowded, the build reports that the table is full or the estimated false-positive rate is too high. Reduce depth or narrow the move set; increase the cap only when you have room. If the problem is insufficient RAM, lower the memory budgets and reduce the depth or goal size as needed.</li>
            <li><strong>Compare the searches you care about.</strong> Save the completed table and retry representative cases with the same moves and settings. Generating a deeper table takes time too, so judge the benefit across the cases you intend to solve.</li>
          </ol>
          <p>Full-position and last-layer Bloom builds reject filters whose estimated false-positive rate exceeds 1%. Their completed filters may be shrunk while retaining all inserted states and keeping the estimated false-positive rate within that limit. A small saved file does not imply that every stage of its build needed equally little RAM. The app checks available memory, but the cap is not a guarantee against running out of memory as the machine’s workload changes.</p>
          <p>For a <strong>partial Bloom table</strong>, Undefine pieces selects the partial goal. <strong>Projection workspace budget (MiB)</strong> controls the temporary coordinate table used to build it; <strong>Memory cap (MiB)</strong> controls the saved Bloom filter. These are separate allocations and can coexist. Reducing the filter cap alone does not reduce the projection workspace. On a memory-constrained machine, consider both budgets.</p>

          <h4>Automatic selection across instances</h4>
          <p>Saved tables are shared across instances; their compatibility depends on moves and goal semantics, rather than the instance name. You do not need to switch back to the instance where a reusable table was generated. A table’s move set must contain <strong>every move enabled in the search</strong>. A table built for fewer moves cannot safely prune a search that allows extra ones.</p>
          <ul>
            <li><strong>Ordinary pattern tables:</strong> the solver considers complete saved move-set pairs alongside the active instance’s fallback set, preferring the smallest set that covers the search moves. Missing tables for the chosen set can be built as needed. Last-layer search considers complete saved last-layer pairs and falls back to the instance’s last-layer tables, rather than building a new last-layer pair just because ordinary tables exist for a smaller move set.</li>
            <li><strong>Full-position and last-layer Bloom tables:</strong> the solver prefers the deepest compatible saved table, then the smaller move set when depths tie. For an explicitly requested search depth, it excludes Bloom tables deeper than that depth. With auto search, a table may never be consulted if a solution is found before its stored depth is reached.</li>
            <li><strong>Dedicated partial tables:</strong> compatible projections are ranked by their entry count, then Bloom depth. The solver can load up to two pattern tables and one partial Bloom table alongside its usual partial heuristics.</li>
          </ul>
          <p>These are selection rules, not a timed comparison of every saved table on your case. “Deeper” or “larger” does not guarantee the fastest result, and two differently shaped move sets are not necessarily comparable just by counting moves. Building a focused table remains useful even when a broader compatible one already exists.</p>

          <h4>Generation controls and GPU acceleration</h4>
          <p>On first launch, the app automatically compares CPU and GPU pruning generation in the background. It checks that both produce the same results and measures the GPU’s transfers and readback as well as compute time. The result is saved locally; a CPU, GPU, or driver change triggers a new comparison. No puzzle tables need to be generated for this check.</p>
          <p>If the GPU is faster, GPU acceleration is enabled by default for a fresh preference and can be turned off in Settings. If CPU generation is faster, no compatible GPU is found, or the check fails, the Settings option is disabled. There are no prompts to enable GPU acceleration in the output area. Individual builds still fall back to CPU when GPU work is unsuitable, retaining completed work.</p>
          <p><strong>Stop</strong> cancels table generation, loading, and saving. Reloading the interface reconnects to an active operation so it can still be stopped. GPU generation uses system RAM as well as device memory.</p>

          <h4>Storage and freeing RAM</h4>
          <p><strong>Settings → Manage storage</strong> lists saved tables, their move sets, and their disk sizes, and lets you delete them. Saved files remain available between sessions. After a solve finishes, loaded tables remain cached in RAM so later searches can reuse them without loading them from disk again. The solver may replace individual caches when a search needs different tables. This avoids repeated loading, especially on slower storage. <strong>Unload pruning table from RAM</strong> releases loaded transition, pruning, and Bloom caches while keeping their saved files; a later search can load them again. Deleting a saved table removes that reusable copy, so it may need to be generated again.</p>
        </section>

        <section id="how-to-batch" tabIndex={-1}>
          <h3>Batch solving</h3>
          <p>Choose <strong>⋮ → Switch to batch solving</strong>. Use the <strong>Input</strong> and <strong>Output</strong> tabs to give either workspace the available height. Starting a batch opens Output; you can switch tabs while it runs.</p>
          <h4>Cases and projects</h4>
          <p>Enter one setup per numbered line in Input. Pasting multiple lines creates multiple cases, each with its own default input orientation. Press <strong>Tab</strong> on a setup line to open its visual editor. Saved visual cases appear as <strong>Case N</strong> links; click one to edit it again. Selecting text across lines and deleting removes cases. Undo and redo restore text edits, including visual entries.</p>
          <p><strong>New batch</strong> creates a project. Use the saved-project dropdown to switch projects, and edit the name to rename the current one. <strong>Delete batch</strong> removes its locally saved cases and progress after confirmation; downloaded files are kept. <strong>Download project</strong> saves a portable copy with progress, and <strong>Open project</strong> restores a copy. Projects also autosave locally.</p>
          <h4>Allowed move sets and priority</h4>
          <p>Each <strong>Allowed move set</strong> card shows only its selected moves. Use <strong>Edit</strong> to change those moves, <strong>Delete</strong> to remove the set, or <strong>Add another move set</strong> to create another independent search. The batch uses one shared instance. Its moves restrict every move-set editor. Changing the instance removes moves outside the new pool and resets progress only for sets whose selected moves change. Existing compatible restricted pruning tables are reused; smaller batches otherwise use the instance’s tables. Large or deep batches can build temporary tables for the current move set. Last-layer searches use dedicated last-layer tables. Use the arrows to put sets in priority order.</p>
          <p>The solver finishes one retained move set before starting the next. Within that set, it searches depths in increasing order and visits cases in input order. This keeps the set’s pruning tables resident in RAM instead of repeatedly loading tables for other sets. The sets remain independent; their moves are never combined into one large search. Duplicate sets keep their earliest priority. A set contained in another is marked as covered and skipped. With Auto depth, a larger set may find a shorter solution, so skipping a contained set does not guarantee that every longer algorithm using only the smaller set will be returned.</p>
          <ul>
            <li><strong>Single solution:</strong> keep one shortest solution within the searched depths from each retained move set for each case, including alternatives that are longer than another set’s solution. Results are sorted by depth, then move-set priority. Identical algorithms are shown once with their originating sets.</li>
            <li><strong>All solutions:</strong> search every retained set independently and collect its alternatives. With Auto, each set searches its own first successful depth; explicit depths collect solutions at the selected lengths. Identical algorithms are shown once, with their originating sets.</li>
          </ul>
          <h4>Temporary tables and memory</h4>
          <p><strong>Temporary tables for large or deep batches</strong> is enabled by default. Preparation starts before a move set when the batch has at least 40 cases or an explicit selected depth is 14 or higher. With Auto and fewer cases, it starts if the search reaches depth 14. These are starting thresholds, not a guarantee that generation will pay for itself on every case. Partial-position searches continue to use their dedicated partial heuristics.</p>
          <p>For full-position searches, the solver reuses an exact saved pruning pair when available. Otherwise, if the memory budget allows, it builds a temporary restricted pair for the current move set. For sets with up to five move types, it also tries successively deeper Bloom tables, until the memory budget or generator limits stop further growth. An explicit search-depth selection also limits growth to its highest selected depth; Auto has no depth-12 cap. The last completed table stays on disk while the next is built. If the filter fills or an allocation cannot fit, generation stops and the last completed table remains usable. Incomplete tables are never used for pruning, so the optimality checks are unchanged.</p>
          <p><strong>Table memory budget (MiB)</strong> can be left blank for Auto. Auto estimates a budget from currently available RAM, including the operating system’s estimate of reclaimable cache, while leaving a reserve for the system. This is an estimate, not a promise that every allocation will succeed. The budget accounts for the pattern arrays and reserves space for Bloom generation buffers; transition tables, search workers, and other applications also use memory. Enter a value to set your own budget. If preparation cannot fit, the solver falls back to reusable tables. Deep Bloom tables can substantially speed up some searches, but generation time and benefit depend on the move set and cases.</p>
          <p>Tables created for this preparation are held in a separate temporary directory and deleted when the move set finishes, pauses, or fails. User-generated saved tables are kept. Resume retains search checkpoints but may rebuild the temporary acceleration tables. Turn off temporary preparation to use saved tables and the instance fallback throughout.</p>
          <h4>Long runs, pause, and deferral</h4>
          <p><strong>Defer after (minutes)</strong> defaults to five minutes. A slow first-pass case can be held at its current depth while the other cases in that move set finish that depth, then resumed without that time limit. All held work at that depth is resolved before the solver advances to the next depth. Set 0 to disable deferral. This changes search order, not the total time allowed for a case or the whole batch. Previously saved ten-second defaults are upgraded to five minutes when migrating an older project.</p>
          <p><strong>Pause</strong> preserves completed depths, solutions, and unfinished search branches separately for each move set. Resume continues that work. Editing a case resets that case’s progress. Editing a set’s moves resets its progress; changing priority retains it. Changes to depth, All solutions, or Last-layer mode reset the search progress. Threads, deferral, and temporary-table settings can change without discarding progress.</p>
          <p>Output can be filtered by case number or setup. <strong>Export CSV</strong> writes solution rows, identifies their move sets, and includes unsolved or invalid cases. <strong>Reset results</strong> clears the saved search progress while keeping the cases and settings.</p>
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
