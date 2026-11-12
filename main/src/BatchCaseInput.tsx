import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState } from "react";
import type { BatchCase } from "./batchProject";
import { casesText, editCases, pointOffset, textEdit, textPoint, type TextSelection } from "./batchCaseText";

export type BatchCaseInputApi = { focusCase(id: string): void };
type Props = {
  entries: BatchCase[]; disabled: boolean; activeCases: Record<string,number>;
  onChange: (entries: BatchCase[]) => void; onEdit: (id: string) => void;
};
type Snapshot = { entries: BatchCase[]; selection: TextSelection };
const ROW = 38;

const BatchCaseInput = forwardRef<BatchCaseInputApi,Props>(function BatchCaseInput({ entries, disabled, activeCases, onChange, onEdit }, ref) {
  const input = useRef<HTMLTextAreaElement>(null);
  const before = useRef<TextSelection>();
  const pending = useRef<TextSelection>();
  const applied = useRef(entries);
  const undo = useRef<Snapshot[]>([]), redo = useRef<Snapshot[]>([]);
  const [highlight, setHighlight] = useState<TextSelection>({ start:0,end:0 });
  const composing = useRef(false);
  const visualDrag = useRef<{ anchor:number; x:number; y:number; moved:boolean }>();
  const skipVisualClick = useRef(false);
  const [scroll, setScroll] = useState({ top:0, left:0 });
  const text = casesText(entries);
  function selection(): TextSelection { return { start:input.current?.selectionStart ?? 0,end:input.current?.selectionEnd ?? 0 }; }
  function commit(next: BatchCase[], selected: TextSelection) {
    applied.current = next; pending.current = selected; onChange(next);
  }
  function reveal(row: number) {
    const area = input.current;
    if (!area) return;
    const top = row * ROW;
    if (top < area.scrollTop) area.scrollTop = top;
    else if (top + ROW + 12 > area.scrollTop + area.clientHeight) area.scrollTop = top + ROW + 12 - area.clientHeight;
    setScroll({ top:area.scrollTop,left:area.scrollLeft });
  }
  useImperativeHandle(ref, () => ({ focusCase(id) {
    const row = entries.findIndex(entry => entry.id === id);
    if (row < 0 || !input.current) return;
    const offset = pointOffset(text,{ row,column:Number.MAX_SAFE_INTEGER });
    input.current.focus(); input.current.setSelectionRange(offset,offset); reveal(row);
  }}));
  useLayoutEffect(() => {
    if (applied.current !== entries) {
      undo.current = []; redo.current = []; applied.current = entries;
    }
    if (pending.current && input.current && !composing.current) {
      input.current.focus(); input.current.setSelectionRange(pending.current.start,pending.current.end);
      reveal(textPoint(text,pending.current.end).row); pending.current = undefined;
    }
  }, [entries,text]);
  useEffect(() => {
    const area = input.current;
    function capture(event: Event) { before.current = { ...selection(),inputType:(event as InputEvent).inputType }; }
    area?.addEventListener("beforeinput",capture);
    return () => area?.removeEventListener("beforeinput",capture);
  }, []);
  function offsetAt(x: number, y: number) {
    const area = input.current;
    if (!area) return 0;
    const rect = area.getBoundingClientRect();
    const style = getComputedStyle(area);
    const context = document.createElement("canvas").getContext("2d");
    if (context) context.font = `${style.fontSize} ${style.fontFamily}`;
    const width = context?.measureText("M").width || 7.8;
    const row = Math.max(0,Math.min(entries.length-1,Math.floor((y-rect.top-6+area.scrollTop)/ROW)));
    return pointOffset(text,{ row,column:Math.max(0,Math.round((x-rect.left+area.scrollLeft)/width)) });
  }
  function dragVisual(x: number, y: number) {
    const drag = visualDrag.current, area = input.current;
    if (!drag || !area) return;
    if (!drag.moved && Math.hypot(x-drag.x,y-drag.y) < 4) return;
    drag.moved = true; skipVisualClick.current = true;
    const tip = offsetAt(x,y);
    area.focus(); area.setSelectionRange(Math.min(drag.anchor,tip),Math.max(drag.anchor,tip),tip < drag.anchor ? "backward" : "forward");
    setHighlight(selection()); reveal(textPoint(text,tip).row);
  }
  function history(backward: boolean) {
    const source = backward ? undo.current : redo.current;
    const target = backward ? redo.current : undo.current;
    const previous = source.pop();
    if (!previous) return;
    target.push({ entries,selection:selection() });
    commit(previous.entries,previous.selection);
  }
  return <div className="batch-case-editor" style={{ height:Math.min(360,Math.max(160,entries.length*ROW+12)) }}>
    <textarea ref={input} className="batch-case-text" aria-label="Case setups" disabled={disabled} value={text}
      wrap="off" spellCheck={false} autoComplete="off" autoCapitalize="off" placeholder="Setup moves"
      onSelect={() => setHighlight(selection())}
      onCompositionStart={() => { composing.current = true; }}
      onCompositionEnd={() => { composing.current = false; pending.current = undefined; }}
      onScroll={e => setScroll({ top:e.currentTarget.scrollTop,left:e.currentTarget.scrollLeft })}
      onChange={e => {
        if (before.current?.inputType === "historyUndo" && undo.current.length) { history(true); return; }
        if (before.current?.inputType === "historyRedo" && redo.current.length) { history(false); return; }
        const value = e.currentTarget.value;
        const after = { start:textPoint(value,e.currentTarget.selectionStart),end:textPoint(value,e.currentTarget.selectionEnd) };
        const next = editCases(entries,textEdit(text,value,before.current));
        undo.current.push({ entries,selection:before.current ?? selection() });
        if (undo.current.length > 100) undo.current.shift();
        redo.current = []; before.current = undefined;
        const displayed = casesText(next);
        commit(next,{ start:pointOffset(displayed,after.start),end:pointOffset(displayed,after.end) });
      }}
      onPaste={() => { before.current = { ...selection(),inputType:"insertFromPaste" }; }}
      onCut={() => { before.current = { ...selection(),inputType:"deleteByCut" }; }}
      onKeyDown={e => {
        before.current = { ...selection(),inputType:e.key === "Backspace" ? "deleteContentBackward" : e.key === "Delete" ? "deleteContentForward" : undefined };
        if ((e.ctrlKey || e.metaKey) && !e.altKey && (e.key.toLowerCase() === "z" || e.key.toLowerCase() === "y")) {
          e.preventDefault(); history(e.key.toLowerCase() === "z" && !e.shiftKey); return;
        }
        if (e.key === "Tab" && !e.shiftKey) {
          e.preventDefault(); const row = textPoint(text,e.currentTarget.selectionStart).row;
          if (entries[row]) onEdit(entries[row].id);
        }
      }} />
    <div className="batch-editor-numbers" aria-hidden="true">
      {entries.map((entry,index) => <span key={entry.id} className="batch-line-number" style={{ top:6+index*ROW-scroll.top }}>{index+1}</span>)}
    </div>
    <div className="batch-editor-visuals">
      {entries.map((entry,index) => entry.facelets && <button key={entry.id} type="button" className={`batch-visual-link${highlight.start < highlight.end && highlight.start <= pointOffset(text,{ row:index,column:Number.MAX_SAFE_INTEGER }) && highlight.end > pointOffset(text,{ row:index,column:0 }) ? " text-selected" : ""}`}
        style={{ top:6+index*ROW-scroll.top,left:-scroll.left }} disabled={disabled}
        onPointerDown={e => {
          if (e.button !== 0) return;
          skipVisualClick.current = false;
          visualDrag.current = { anchor:e.shiftKey ? (input.current?.selectionDirection === "backward" ? selection().end : selection().start) : offsetAt(e.clientX,e.clientY),x:e.clientX,y:e.clientY,moved:e.shiftKey };
          e.currentTarget.setPointerCapture(e.pointerId);
          if (e.shiftKey) { e.preventDefault(); dragVisual(e.clientX,e.clientY); }
        }}
        onPointerMove={e => { if (visualDrag.current) dragVisual(e.clientX,e.clientY); }}
        onPointerUp={() => { visualDrag.current = undefined; }}
        onPointerCancel={() => { visualDrag.current = undefined; skipVisualClick.current = true; }}
        onClick={() => { if (skipVisualClick.current) skipVisualClick.current = false; else onEdit(entry.id); }}>Case {index+1}</button>)}
    </div>
    <div className="batch-editor-status">
      {entries.map((entry,index) => <div className={`batch-editor-row-status${entry.error ? " has-error" : ""}`} key={entry.id} style={{ top:6+index*ROW-scroll.top }}>
        <span className="batch-case-badge" title={entry.error ?? `${entry.completedDepths.length} depths completed`}>
          {entry.error ? "Invalid" : activeCases[entry.id] !== undefined ? `Searching d${activeCases[entry.id]}` : entry.deferredDepth != null ? `Deferred d${entry.deferredDepth}` : entry.solutions.length ? `${entry.solutions.length} found` : entry.completedDepths.length ? `d${Math.max(...entry.completedDepths)}` : ""}
        </span>
        <button className="batch-remove" aria-label={`Remove case ${index+1}`} disabled={disabled} onClick={() => {
          const next = entries.length === 1 ? editCases(entries,{ start:0,end:text.length,text:"" }) : entries.filter(c => c.id !== entry.id);
          undo.current.push({ entries,selection:selection() }); redo.current = [];
          const offset = pointOffset(casesText(next),{ row:Math.min(index,next.length-1),column:0 });
          commit(next,{start:offset,end:offset});
        }}>×</button>
      </div>)}
    </div>
  </div>;
});
export default BatchCaseInput;
