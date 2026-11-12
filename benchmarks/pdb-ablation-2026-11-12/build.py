import subprocess,pathlib,re,json,shutil,time
root=pathlib.Path('/tmp/fto-ablation-20261112'); out=pathlib.Path('/tmp/fto-ablation-results-20261112');target=pathlib.Path('/home/abid/Documents/GitHub/FTO/Optimal-FTO-Solver/main/target')
files=['main/crates/fto-core/src/pruning.rs','main/crates/fto-core/src/search.rs','main/crates/fto-core/src/search/resume.rs']
for variant in ['baseline','early','batch','both']:
 start=time.monotonic()
 for name in files:
  text=subprocess.check_output(['git','show',f'{"78d6ae2" if variant=="baseline" else "ff61193"}:{name}'],cwd=root,text=True)
  if name.endswith('/pruning.rs') and variant=='early':
   text,n=re.subn(r'pub\(crate\) fn can_batch\(&self\) -> bool \{.*?\n    \}', 'pub(crate) fn can_batch(&self) -> bool { false\n    }',text,flags=re.S);assert n==2,n
  if name.endswith('/pruning.rs') and variant=='batch':
   assert text.count('if value > bound {')==1
   assert text.count('if first > bound {')==2
   text=text.replace('if value > bound {','if false && value > bound {').replace('if first > bound {','if false && first > bound {')
  (root/name).write_text(text)
  snap=out/'sources'/variant/name;snap.parent.mkdir(parents=True,exist_ok=True);snap.write_text(text)
 print('BUILD',variant,flush=True)
 p=subprocess.run(['cargo','build','--release','-p','fto-core','--no-default-features','-j','1','--target-dir',str(target),'--message-format=json'],cwd=root/'main',capture_output=True,text=True)
 (out/f'build-{variant}.stderr').write_text(p.stderr)
 if p.returncode:raise RuntimeError(p.stderr)
 artifacts=[json.loads(line) for line in p.stdout.splitlines() if line.startswith('{')]
 rlib=next(f for a in artifacts if a.get('reason')=='compiler-artifact' and a['target']['name']=='fto_core' for f in a['filenames'] if f.endswith('.rlib'))
 shutil.copy2(rlib,out/f'libfto_core-{variant}.rlib')
 print('BUILT',variant,round(time.monotonic()-start,2),flush=True)
