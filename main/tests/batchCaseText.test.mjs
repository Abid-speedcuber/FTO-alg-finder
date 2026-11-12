import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
import ts from 'typescript';
const compile = path => ts.transpileModule(readFileSync(new URL(path,import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2020,module:ts.ModuleKind.ESNext}}).outputText;
const url = source => `data:text/javascript;base64,${Buffer.from(source).toString('base64')}`;
const projectUrl = url(compile('../src/batchProject.ts'));
const {newCase} = await import(projectUrl);
const {casesText,editCases,textEdit,pointOffset,textPoint} = await import(url(compile('../src/batchCaseText.ts').replace('"./batchProject"',JSON.stringify(projectUrl))));
function checked(setup) {return {...newCase(setup),completedDepths:[2,3],firstDepth:3,solutions:[{depth:3,algorithm:"R'"}],checkpoint:{version:1,depth:4,input:[],tasks:[]}};}
test('deleting whole lines preserves the correct untouched duplicate cases and checkpoints',()=>{
  const entries=[checked('R'),checked('R'),checked('F')];
  const edit=textEdit(casesText(entries),'R\nF',{start:0,end:2,inputType:'deleteContentBackward'});
  const next=editCases(entries,edit);
  assert.deepEqual(next,[entries[1],entries[2]]); assert.equal(next[0],entries[1]);
});
test('multi-line paste replaces a selected range and preserves surrounding cases',()=>{
  const entries=[checked('U'),checked('R'),checked('F'),checked('L')];
  const next=editCases(entries,{start:2,end:6,text:"BR\r\nBL\r\nD\n"});
  assert.deepEqual(next.map(c=>c.setup),['U','BR','BL','D','L']);
  assert.equal(next[0],entries[0]); assert.equal(next[4],entries[3]);
  assert.ok(next.slice(1,4).every(c=>c.checkpoint===null && !c.solutions.length && !c.completedDepths.length));
});
test('untouched visual cases survive renumbering; editing a visual token replaces its state',()=>{
  const visual={...checked('old setup'),facelets:Array(72).fill(0)};
  const entries=[checked('U'),visual,checked('F')];
  const next=editCases(entries,{start:0,end:2,text:''});
  assert.equal(next[0],visual); assert.equal(casesText(next),'Case 1\nF');
  const edited=editCases(next,{start:0,end:6,text:'R U'});
  assert.equal(edited[0].setup,'R U'); assert.equal(edited[0].facelets,undefined); assert.equal(edited[0].checkpoint,null);
  assert.equal(edited[1],entries[2]);
});
test('partial-line replacement merges boundaries and keeps subsequent cases intact',()=>{
  const entries=[checked('R U'),checked('F L'),checked('BR')];
  const next=editCases(entries,{start:2,end:6,text:'D'});
  assert.deepEqual(next.map(c=>c.setup),['R DL','BR']); assert.equal(next[1],entries[2]);
  assert.equal(next[0].checkpoint,null);
});
test('Backspace at a line boundary joins text; deleting everything leaves one blank case',()=>{
  const entries=[checked('R'),checked('U')];
  assert.deepEqual(editCases(entries,textEdit('R\nU','RU',{start:2,end:2,inputType:'deleteContentBackward'})).map(c=>c.setup),['RU']);
  const empty=editCases(entries,{start:0,end:3,text:''}); assert.equal(empty.length,1); assert.equal(empty[0].setup,'');
});
test('caret positions survive renumbering of visual references across digit boundaries',()=>{
  const entries=Array.from({length:10},(_,i)=>i===9?{...newCase(),facelets:Array(72).fill(0)}:newCase('R'));
  const old=casesText(entries), raw=old.slice(2), next=casesText(editCases(entries,{start:0,end:2,text:''}));
  assert.ok(raw.endsWith('Case 10')); assert.ok(next.endsWith('Case 9'));
  assert.equal(pointOffset(next,textPoint(raw,raw.length)),next.length);
});
