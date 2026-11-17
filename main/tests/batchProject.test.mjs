import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import ts from 'typescript';
const compiled = ts.transpileModule(readFileSync(new URL('../src/batchProject.ts', import.meta.url),'utf8'), { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ESNext } }).outputText;
const { createProject, newCase, clearCase, projectCsv, parseProject, moveSetsOf, effectiveMoveSets, coveringMoveSet, progressForSet, mergeSetProgress, changeMoveSets, nextBatchAttempt, emptyProgress, constrainBatchInstance } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
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
  assert.equal(rows.length,5); assert.ok(rows.every(r=>r.length===13));
  assert.equal(rows[1][1],'R, "U"\nF'); assert.equal(rows[1][3],"R' U'");
  assert.equal(rows[2][2],'3'); assert.equal(rows[3][1],"'=DANGEROUS()");
  assert.deepEqual(JSON.parse(rows[4][1]).facelets,p.cases[2].facelets);
});
test('project round trip keeps progress, moves and centers; running imports become paused copies', () => {
  const p=createProject(['R'],['R','U']); p.status='running';
  p.cases=[{...newCase('R'),completedDepths:[0,1],firstDepth:1,solutions:[{depth:1,algorithm:"R'"}],nodes:47}];
  const imported=parseProject(JSON.stringify(p));
  assert.notEqual(imported.id,p.id); assert.equal(imported.status,'paused'); assert.deepEqual(imported.cases,p.cases); assert.deepEqual(imported.settings,p.settings);
  assert.deepEqual(clearCase(p.cases[0]),{...p.cases[0],completedDepths:[],firstDepth:null,solutions:[],nodes:0,error:null,deferredDepth:null,checkpoint:null,moveSetResults:undefined});
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
  assert.equal(restored.cases[0].deferredDepth,7); assert.equal(restored.settings.slowCaseSeconds,300);
  assert.equal(csv(projectCsv(p))[1][11],'7'); assert.equal(clearCase(p.cases[0]).deferredDepth,null);
  delete p.settings.slowCaseSeconds; assert.equal(parseProject(JSON.stringify(p)).settings.slowCaseSeconds,300);
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


test('entitled center slot sets survive batch round trips and reject invalid home slots', () => {
  const p = createProject(['R'], ['R']);
  p.cases[0].facelets = Array(72).fill(8);
  p.cases[0].centerTargets = { uf:[null,null,null,null], rl:[null,null,null,null], ufSources:[null,null,null,null], rlSources:[null,null,null,null], rlTopSources:[[],[],[],[]], ufSlots:[[],[],[6,7],[]], rlSlots:[[],[3],[],[9,11]] };
  assert.deepEqual(parseProject(JSON.stringify(p)).cases[0].centerTargets, p.cases[0].centerTargets);
  for (const invalid of [[[3],[],[],[9]], [[],[],[5],[]], [[],[],[6,6],[]], [[],[],[12],[]], [[],[]]]) {
    const copy = structuredClone(p); copy.cases[0].centerTargets.ufSlots = invalid;
    assert.throws(() => parseProject(JSON.stringify(copy)));
  }
  delete p.cases[0].centerTargets.ufSlots; delete p.cases[0].centerTargets.rlSlots;
  assert.doesNotThrow(() => parseProject(JSON.stringify(p)));
});


test('ignored sticker colors and last-layer roles survive portable projects', () => {
  const p = createProject(['R'], ['R']);
  p.cases[0].facelets = Array(72).fill(8);
  p.cases[0].ignoredStickerState = Array(72).fill(null);
  p.cases[0].ignoredStickerState[47] = {color: 5, mark: 'fixed'};
  const restored = parseProject(JSON.stringify(p));
  assert.deepEqual(restored.cases[0].ignoredStickerState, p.cases[0].ignoredStickerState);
  assert.deepEqual(JSON.parse(csv(projectCsv(p))[1][1]).ignoredStickerState, p.cases[0].ignoredStickerState);
  assert.deepEqual(clearCase(restored.cases[0]).ignoredStickerState, p.cases[0].ignoredStickerState);
  for (const invalid of [[], Array(72).fill({color: 8, mark: null}), Array(72).fill({color: 0, mark: 'unknown'})]) {
    const copy = structuredClone(p); copy.cases[0].ignoredStickerState = invalid;
    assert.throws(() => parseProject(JSON.stringify(copy)), /remembered sticker/);
  }
  delete p.cases[0].ignoredStickerState;
  assert.doesNotThrow(() => parseProject(JSON.stringify(p)), 'older projects remain compatible');
});


const set = (id, moves) => ({id, moves, instanceMoves:[...moves]});
const progress = (depth, algorithm, extra={}) => ({completedDepths:[depth],firstDepth:depth,solutions:[{depth,algorithm}],nodes:20,error:null,deferredDepth:null,checkpoint:null,...extra});
test('contained sets are skipped, duplicate sets keep their earliest priority, and moves are never unioned', () => {
  const sets=[set('small',['R','U']),set('right',['R','U','Rw']),set('front',['R','U','F']),set('duplicate',['U','R','Rw'])];
  assert.deepEqual(effectiveMoveSets(sets).map(s=>s.id),['right','front']);
  assert.equal(coveringMoveSet(sets,0).id,'right');
  assert.equal(coveringMoveSet(sets,3).id,'right');
  assert.ok(effectiveMoveSets(sets).every(s=>!(s.moves.includes('Rw')&&s.moves.includes('F'))));
});
test("single-solution mode keeps each set's shortest result and sorts by depth and priority", () => {
  const sets=[set('one',['R']),set('two',['U'])];
  let c=newCase('R');
  c=mergeSetProgress(c,'one',progress(7,'R U'),sets,false);
  c=mergeSetProgress(c,'two',progress(5,'U R'),sets,false);
  assert.deepEqual(c.solutions,[{depth:5,algorithm:'U R',moveSetIds:['two']},{depth:7,algorithm:'R U',moveSetIds:['one']}]);
  assert.equal(c.moveSetResults.one.firstDepth,7);
  c=mergeSetProgress(c,'one',progress(5,'R U'),sets,false);
  assert.equal(c.solutions[0].algorithm,'R U');
  const project=createProject(['R'],['R']);project.settings.all=false;project.settings.moveSets=sets;project.cases=[c];
  assert.equal(changeMoveSets(project,[sets[1],sets[0]]).cases[0].solutions[0].algorithm,'U R');
});
test('all solutions merge duplicates while retaining their originating sets and independent checkpoints', () => {
  const sets=[set('one',['R']),set('two',['U'])];
  const cp={version:1,depth:6,input:[],tasks:[]};
  let c=mergeSetProgress(newCase('R'),'one',progress(5,'R U',{checkpoint:cp,deferredDepth:6}),sets);
  c=mergeSetProgress(c,'two',progress(5,'R U'),sets);
  assert.deepEqual(c.solutions,[{depth:5,algorithm:'R U',moveSetIds:['one','two']}]);
  assert.deepEqual(progressForSet(c,'one','one').checkpoint,cp);
  assert.equal(progressForSet(c,'two','one').checkpoint,null);
  const p=createProject(['R'],['R']);p.settings.moveSets=sets;p.cases=[c];
  const restored=parseProject(JSON.stringify(p));assert.deepEqual(restored.cases[0].moveSetResults,c.moveSetResults);
  assert.match(csv(projectCsv(p))[1][12],/1.*2/);
  assert.deepEqual(changeMoveSets(p,[sets[1],sets[0]]).cases[0].moveSetResults,c.moveSetResults);
  const edited=changeMoveSets(p,[set('one',['R','F']),sets[1]]).cases[0];
  assert.equal(edited.moveSetResults.one,undefined);assert.deepEqual(edited.moveSetResults.two,c.moveSetResults.two);
  assert.equal(clearCase(c).moveSetResults,undefined);
});
test('legacy single-set progress migrates without losing unfinished search work', () => {
  const p=createProject(['R'],['R']);p.version=1;delete p.settings.moveSets;
  p.cases=[{...newCase('R'),...progress(3,'R',{checkpoint:{version:1,depth:4,input:[],tasks:[]}})}];
  assert.equal(parseProject(JSON.stringify(p)).version,1);
  const sets=moveSetsOf(p.settings);
  const next=changeMoveSets(p,[...sets,set('other',['U'])]);
  assert.equal(next.version,2);
  assert.deepEqual(progressForSet(next.cases[0],'legacy','legacy').checkpoint,p.cases[0].checkpoint);
  assert.deepEqual(progressForSet(next.cases[0],'other','legacy').solutions,[]);
});
test('malformed move sets and nested progress cannot be imported', () => {
  const p=createProject(['R'],['R']);
  for(const mutate of [p=>p.settings.moveSets.push(p.settings.moveSets[0]),p=>p.settings.moveSets[0].moves=['F'],
    p=>p.cases[0].moveSetResults={one:{...progress(2,'R'),completedDepths:[256]}},
    p=>p.cases[0].moveSetResults={one:{...progress(2,'R'),checkpoint:{version:1,depth:4,input:[],tasks:[{}]}}}]) {
    const copy=structuredClone(p);mutate(copy);assert.throws(()=>parseProject(JSON.stringify(copy)));
  }
});

function scheduledProject() {
  const p=createProject(['R'],['R']);
  p.settings.moveSets=[{id:'a',moves:['R'],instanceMoves:['R']},{id:'b',moves:['U'],instanceMoves:['U']}];
  p.cases=[newCase('R'),newCase('U')];
  return p;
}
function finishAttempt(p, attempt, patch={}) {
  p.cases=p.cases.map(entry=>entry.id!==attempt.caseId ? entry : mergeSetProgress(entry,attempt.moveSetId,{
    ...progressForSet(entry,attempt.moveSetId,'a'),completedDepths:[...progressForSet(entry,attempt.moveSetId,'a').completedDepths,attempt.depth],...patch,
  },p.settings.moveSets,p.settings.all));
}
test('scheduler finishes each move set in depth and case order without unioning moves',()=>{
  const p=scheduledProject(), seen=[];
  let attempt;
  while((attempt=nextBatchAttempt(p,[1,2]))) {
    seen.push([attempt.depth,p.cases.findIndex(c=>c.id===attempt.caseId)+1,attempt.moveSetId]);
    finishAttempt(p,attempt);
  }
  assert.deepEqual(seen,[[1,1,'a'],[1,2,'a'],[2,1,'a'],[2,2,'a'],[1,1,'b'],[1,2,'b'],[2,1,'b'],[2,2,'b']]);
});
test('deferred cases yield within their set and block that set from advancing',()=>{
  const p=scheduledProject(); const held=nextBatchAttempt(p,[1,2]);
  p.cases[0]=mergeSetProgress(p.cases[0],'a',{...emptyProgress(),deferredDepth:1,checkpoint:{version:1,depth:1,input:[],tasks:[]}},p.settings.moveSets);
  const seen=[];
  for(let i=0;i<1;i++) {const attempt=nextBatchAttempt(p,[1,2]);seen.push([attempt.caseId,attempt.moveSetId,attempt.depth]);finishAttempt(p,attempt);}
  assert.deepEqual(seen,[[p.cases[1].id,'a',1]]);
  assert.deepEqual(nextBatchAttempt(p,[1,2]),held);
  const restored=parseProject(JSON.stringify(p));
  assert.deepEqual(nextBatchAttempt(restored,[1,2]),held,'pause/export/resume retains the depth barrier');
  finishAttempt(p,held,{checkpoint:null,deferredDepth:null});
  assert.equal(nextBatchAttempt(p,[1,2]).depth,2);
});
test('single mode searches every set for its own shortest result, including longer alternatives',()=>{
  const p=scheduledProject();p.settings.all=false;p.cases=p.cases.slice(0,1);
  let attempt=nextBatchAttempt(p,[1,2,3]);
  finishAttempt(p,attempt,{firstDepth:1,solutions:[{depth:1,algorithm:"R'"}]});
  attempt=nextBatchAttempt(p,[1,2,3]);assert.equal(attempt.moveSetId,'b');assert.equal(attempt.depth,1);
  finishAttempt(p,attempt);
  attempt=nextBatchAttempt(p,[1,2,3]);assert.equal(attempt.depth,2);
  finishAttempt(p,attempt,{firstDepth:2,solutions:[{depth:2,algorithm:"U R"}]});
  assert.equal(nextBatchAttempt(p,[1,2,3]),null);
  assert.deepEqual(p.cases[0].solutions.map(s=>s.algorithm),["R'","U R"]);
});
test('Auto All finishes each set at its own first successful depth; explicit All includes longer depths',()=>{
  const p=scheduledProject();p.cases=p.cases.slice(0,1);
  finishAttempt(p,{caseId:p.cases[0].id,moveSetId:'a',depth:0},{firstDepth:0,solutions:[{depth:0,algorithm:''}]});
  let attempt=nextBatchAttempt(p,null);assert.equal(attempt.moveSetId,'b');assert.equal(attempt.depth,0);
  finishAttempt(p,attempt);
  attempt=nextBatchAttempt(p,null);assert.equal(attempt.moveSetId,'b');assert.equal(attempt.depth,1);
  finishAttempt(p,attempt,{firstDepth:1,solutions:[{depth:1,algorithm:'U'}]});
  assert.equal(nextBatchAttempt(p,null),null);
  assert.deepEqual(nextBatchAttempt(p,[0,1,2]),{caseId:p.cases[0].id,moveSetId:'a',depth:1});
});
test('scheduler skips covered sets, blank cases, completed work and errors',()=>{
  const p=scheduledProject();p.settings.moveSets.push({id:'covered',moves:['R'],instanceMoves:['R']});p.cases.push(newCase());
  p.cases[0]=mergeSetProgress(p.cases[0],'a',{...emptyProgress(),error:'Invalid state'},p.settings.moveSets);
  let attempt;
  const seen=[];
  while((attempt=nextBatchAttempt(p,[2,4]))) {seen.push(attempt);finishAttempt(p,attempt);}
  assert.equal(seen.length,6);assert.ok(seen.every(a=>a.moveSetId!=='covered'&&a.caseId!==p.cases[2].id));
  assert.deepEqual(seen.map(a=>a.depth),[2,4,2,2,4,4]);
});

test('one batch instance constrains every editor, clips incompatible sets, and preserves compatible checkpoints',()=>{
  const p=scheduledProject();p.settings.instanceMoves=['R','U'];
  const checkpoint={version:1,depth:2,input:[0],tasks:[]};
  p.cases[0]=mergeSetProgress(p.cases[0],'a',{...emptyProgress(),completedDepths:[0,1],checkpoint},p.settings.moveSets);
  p.cases[0]=mergeSetProgress(p.cases[0],'b',{...emptyProgress(),completedDepths:[0,1],checkpoint},p.settings.moveSets);
  const changed=constrainBatchInstance(p,['R','F'],'restricted');
  assert.deepEqual(changed.settings.instanceMoves,['R','F']);assert.equal(changed.settings.instanceId,'restricted');
  assert.deepEqual(changed.settings.moveSets.map(set=>set.moves),[['R'],[]]);
  assert.ok(changed.settings.moveSets.every(set=>set.instanceId==='restricted'&&set.instanceMoves.join(' ')==='R F'));
  assert.deepEqual(progressForSet(changed.cases[0],'a','a').checkpoint,checkpoint);
  assert.equal(progressForSet(changed.cases[0],'b','a').checkpoint,null);
  assert.equal(parseProject(JSON.stringify(changed)).settings.instanceId,'restricted');
  const empty=constrainBatchInstance(changeMoveSets(changed,[]),['R','F'],'restricted');
  assert.deepEqual(empty.settings.instanceMoves,['R','F'],'deleting the final set does not erase the batch instance');
});

test('single mode reports just the shortest algorithm from each set, merging identical algorithms across sets',()=>{
 const sets=[set('one',['R']),set('two',['U'])];
 let c=mergeSetProgress(newCase('R'),'one',{...emptyProgress(),firstDepth:2,solutions:[{depth:4,algorithm:'longer'},{depth:2,algorithm:'short'},{depth:2,algorithm:'other'}]},sets,false);
 c=mergeSetProgress(c,'two',{...emptyProgress(),firstDepth:2,solutions:[{depth:2,algorithm:'short'}]},sets,false);
 assert.deepEqual(c.solutions,[{depth:2,algorithm:'short',moveSetIds:['one','two']}]);
});
