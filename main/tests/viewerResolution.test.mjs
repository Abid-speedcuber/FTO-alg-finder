import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';

for (const path of ['../public/fto-viewer/fto-viewer.js','../../public/fto-viewer/fto-viewer.js']) {
  const source=readFileSync(new URL(path,import.meta.url),'utf8');
  const start=source.indexOf('    function resize() {');
  const body=source.slice(start,source.indexOf('    applyFaceColors(options.faceColors, false);',start));
  test(`${path}: canvas fits its host at every app zoom and matches physical pixel density`,()=>{
    for(const zoom of [0.5,0.7,0.9,1,1.1,1.5,2,3]) for(const dpr of [1,1.25,2,3]) {
      const host={clientWidth:620,clientHeight:459};
      const canvas={parentElement:host,style:{},width:300,height:150,getBoundingClientRect(){return {width:parseFloat(this.style.width)*zoom};}};
      let allocations=0;
      const context=vm.createContext({canvas,options:{},window:{devicePixelRatio:dpr},renderer:{setSize(w,h){allocations++;canvas.width=w;canvas.height=h;}},render(){}});
      vm.runInContext(body,context);context.resize();
      assert.equal(parseFloat(canvas.style.width),459);
      assert.equal(parseFloat(canvas.style.height),459);
      assert.equal(canvas.width,Math.round(459*zoom*dpr));
      assert.equal(canvas.height,canvas.width);
      context.resize();assert.equal(allocations,1,'unchanged dimensions should reuse the buffer');
      host.clientHeight=510;context.resize();assert.equal(parseFloat(canvas.style.width),510);
      host.clientWidth=0;host.clientHeight=0;context.resize();assert.equal(parseFloat(canvas.style.width),510,'hidden viewers retain their last size');
    }
  });
}
