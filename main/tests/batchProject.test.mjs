import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import ts from 'typescript';
const compiled = ts.transpileModule(readFileSync(new URL('../src/batchProject.ts', import.meta.url),'utf8'), { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ESNext } }).outputText;
const { createProject, newCase, clearCase, projectCsv, parseProject } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
function csv(text) {
  // Parse quoted fields, including embedded commas, newlines and escaped quotes.
  const rows=[]; let row=[], field='', quoted=false;
  for(let i=1; i<text.length;i++) { const c=text[i];
    if(c==='"') { if(quoted && text[i+1]==='"') {field+='"';i++;} else quoted=!quoted; }
    else if(!quoted && c===',') {row.push(field);field='';}
    else if(!quoted && c==='\r' && text[i+1]==='\n') {row.push(field);rows.push(row);row=[];field='';i++;}
    else field+=c;
  }
  return rows;
}
test('CSV has one row per solution and retains unsolved cases and visual states', () => {
  const p=createProject(['R','U'],['R','U']);
  p.cases=[{...newCase('R, "U"\nF'),firstDepth:2,completedDepths:[1,2],solutions:[{depth:2,algorithm:"R' U'"},{depth:3,algorithm:"U R U"}]},newCase('=DANGEROUS()'),{...newCase(),facelets:Array.from({length:72},(_,i)=>Math.floor(i/9))},newCase()];
  const rows=csv(projectCsv(p));
  assert.equal(rows.length,5); assert.ok(rows.every(r=>r.length===12));
  assert.equal(rows[1][1],'R, "U"\nF'); assert.equal(rows[1][3],"R' U'");
  assert.equal(rows[2][2],'3'); assert.equal(rows[3][1],"'=DANGEROUS()");
  assert.deepEqual(JSON.parse(rows[4][1]).facelets,p.cases[2].facelets);
});
test('project round trip keeps progress, moves and centers; running imports become paused copies', () => {
  const p=createProject(['R'],['R','U']); p.status='running';
  p.cases=[{...newCase('R'),completedDepths:[0,1],firstDepth:1,solutions:[{depth:1,algorithm:"R'"}],nodes:47}];
  const imported=parseProject(JSON.stringify(p));
  assert.notEqual(imported.id,p.id); assert.equal(imported.status,'paused'); assert.deepEqual(imported.cases,p.cases); assert.deepEqual(imported.settings,p.settings);
  assert.deepEqual(clearCase(p.cases[0]),{...p.cases[0],completedDepths:[],firstDepth:null,solutions:[],nodes:0,error:null,deferredDepth:null,checkpoint:null});
});
test('malformed and duplicate imported checkpoints are rejected', () => {
  const p=createProject(['R'],['R']);
  for(const change of [p=>p.cases.push(p.cases[0]), p=>p.cases[0].completedDepths=[256], p=>p.cases[0].facelets=[1], p=>p.settings.threads=0, p=>p.settings.all='true', p=>p.cases[0].centerTargets={}]) {
    const copy=structuredClone(p); change(copy); assert.throws(()=>parseProject(JSON.stringify(copy)));
  }
});

test('deferred depths survive projects/CSV and reset on case edits; old projects get default budget', () => {
  const p=createProject(['R'],['R']); p.cases=[{...newCase('R'),deferredDepth:7,completedDepths:[5,6]}];
  const restored=parseProject(JSON.stringify(p));
  assert.equal(restored.cases[0].deferredDepth,7); assert.equal(restored.settings.slowCaseSeconds,10);
  assert.equal(csv(projectCsv(p))[1][11],'7'); assert.equal(clearCase(p.cases[0]).deferredDepth,null);
  delete p.settings.slowCaseSeconds; assert.equal(parseProject(JSON.stringify(p)).settings.slowCaseSeconds,10);
  p.settings.slowCaseSeconds=-1; assert.throws(()=>parseProject(JSON.stringify(p)));
});

test('DFS stacks survive portable project export and case edits clear them', () => {
  const p=createProject(['R'],['R']);
  const cp={version:1,depth:4,input:[0,0,1,12],tasks:[{root:0,base:0,path:[12],next:[1,0],entered:[true,false]}]};
  p.cases=[{...newCase('R'),checkpoint:cp}];
  assert.deepEqual(parseProject(JSON.stringify(p)).cases[0].checkpoint,cp);
  assert.equal(clearCase(p.cases[0]).checkpoint,null);
  for(const update of [cp=>cp.version=2,cp=>cp.tasks[0].path=[255],cp=>cp.tasks[0].next=[],cp=>cp.tasks[0].base=2]) {
    const copy=structuredClone(p);update(copy.cases[0].checkpoint);assert.throws(()=>parseProject(JSON.stringify(copy)));
  }
});
