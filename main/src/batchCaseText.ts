import { newCase, type BatchCase } from "./batchProject";

export const caseLine = (entry: BatchCase, index: number) => entry.facelets ? `Case ${index + 1}` : entry.setup;
export const casesText = (entries: BatchCase[]) => entries.map(caseLine).join("\n");
export type TextEdit = { start: number; end: number; text: string };
export type TextSelection = { start: number; end: number; inputType?: string };

/** Recover the browser's edit, using its original selection to disambiguate repeated lines. */
export function textEdit(previous: string, next: string, selection?: TextSelection): TextEdit {
  if (selection) {
    let { start, end } = selection;
    const delta = next.length - previous.length;
    if (start === end && delta < 0) {
      if (selection.inputType?.includes("Backward")) start += delta;
      else if (selection.inputType?.includes("Forward")) end -= delta;
    }
    const length = delta + end - start;
    if (start >= 0 && end <= previous.length && length >= 0 &&
        previous.slice(0,start) === next.slice(0,start) && previous.slice(end) === next.slice(start+length)) {
      return { start, end, text: next.slice(start,start+length) };
    }
  }
  let start = 0;
  while (start < previous.length && start < next.length && previous[start] === next[start]) start++;
  let end = previous.length, tail = next.length;
  while (end > start && tail > start && previous[end-1] === next[tail-1]) { end--; tail--; }
  return { start, end, text: next.slice(start,tail) };
}

/** Preserve untouched cases by their original spans, including duplicate setups and visual states. */
export function editCases(entries: BatchCase[], edit: TextEdit): BatchCase[] {
  const previous = casesText(entries);
  const inserted = edit.text.replace(/\r\n?/g,"\n");
  const next = previous.slice(0,edit.start) + inserted + previous.slice(edit.end);
  const lines = next.split("\n");
  const starts: number[] = [];
  let offset = 0;
  for (const line of lines) { starts.push(offset); offset += line.length + 1; }
  const indexAt = new Map(starts.map((start,index) => [start,index]));
  const retained = new Map<number,BatchCase>();
  const delta = inserted.length - (edit.end - edit.start);
  offset = 0;
  entries.forEach((entry,index) => {
    const line = caseLine(entry,index), end = offset + line.length;
    const candidates: number[] = [];
    if (end <= edit.start) candidates.push(offset);
    if (offset >= edit.end) candidates.push(offset + delta);
    for (const mapped of candidates) {
      const nextIndex = indexAt.get(mapped);
      if (nextIndex !== undefined && !retained.has(nextIndex) && lines[nextIndex] === line) {
        retained.set(nextIndex,entry); break;
      }
    }
    offset = end + 1;
  });
  return lines.map((line,index) => retained.get(index) ?? newCase(line));
}

export function textPoint(text: string, offset: number) {
  const before = text.slice(0,offset);
  const row = before.split("\n").length - 1;
  return { row, column: offset - (before.lastIndexOf("\n") + 1) };
}
export function pointOffset(text: string, point: { row: number; column: number }) {
  const lines = text.split("\n");
  const row = Math.min(point.row,lines.length-1);
  return lines.slice(0,row).reduce((sum,line) => sum + line.length + 1,0) + Math.min(point.column,lines[row].length);
}
