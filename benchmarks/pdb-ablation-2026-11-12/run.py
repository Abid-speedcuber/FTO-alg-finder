import pathlib,subprocess,json,os,time,re,statistics,math
root=pathlib.Path('/tmp/fto-ablation-results-20261112')
# Fixed depths are checked into depths-*.json; calibration is not rerun.
reference={};allrows=[]
orders={'ll6':['batch','baseline','both','early'],'ll20':['early','both','baseline','batch'],'full20':['both','batch','early','baseline']}
for family,variants in orders.items():
 for variant in variants:
  print('RUN',family,variant,flush=True)
  with (root/f'{family}-{variant}.out').open('w')as stdout,(root/f'{family}-{variant}.err').open('w')as stderr:
   p=subprocess.run([str(root/variant),family,'4'],env={**os.environ,'FTO_BENCH_VARIANT':variant},stdout=stdout,stderr=stderr,timeout=180)
  assert p.returncode==0,(family,variant,p.returncode)
  rows=[json.loads(line)for line in (root/f'{family}-{variant}.out').read_text().splitlines()]
  for r in rows:
   key=(r['family'],r['case'],r['depth']);sig=(r['nodes'],r['solutions'],r['hash'])
   if key in reference:assert reference[key]==sig,(key,variant,'RESULT MISMATCH',reference[key],sig)
   else:reference[key]=sig
   r['variant']=variant;allrows.append(r)
  for case in dict.fromkeys(r['case']for r in rows):
   values=[r['ms']for r in rows if r['case']==case and r['round']>0]
   r=next(r for r in rows if r['case']==case)
   print(f"{family} {variant} {case} d={r['depth']} median={statistics.median(values):.3f} nodes={r['nodes']} algs={r['solutions']}",flush=True)
  (root/'measurements.json').write_text(json.dumps(allrows,indent=2))
# Exact ordered algorithm files, not just their hashes.
for family,variants in orders.items():
 for case in dict.fromkeys(r['case']for r in allrows if r['family']==family):
  expected=(root/'algs'/f'baseline-{family}-{case}.txt').read_bytes()
  for variant in variants:assert (root/'algs'/f'{variant}-{family}-{case}.txt').read_bytes()==expected
summary=[]
for key in reference:
 family,case,depth=key
 stats={v:statistics.median([r['ms']for r in allrows if (r['family'],r['case'],r['depth'])==key and r['variant']==v and r['round']>0])for v in orders[family]}
 row=dict(family=family,case=case,depth=depth,nodes=reference[key][0],solutions=reference[key][1],ms=stats,percent_less_time={v:100*(1-stats[v]/stats['baseline'])for v in ['early','batch','both']});summary.append(row)
(root/'summary.json').write_text(json.dumps(summary,indent=2))
print('ALL ORDERED ALGORITHMS AND NODE COUNTS MATCH',flush=True)
for fam in orders:
 rows=[r for r in summary if r['family']==fam and r['nodes']>100]
 print('FAMILY',fam,'cases',len(rows),flush=True)
 for v in ['early','batch','both']:
  ratios=[r['ms'][v]/r['ms']['baseline']for r in rows]
  print(v,'geomean_less_pct',round(100*(1-math.exp(sum(math.log(x)for x in ratios)/len(ratios))),2),'min',round(min(r['percent_less_time'][v]for r in rows),2),'max',round(max(r['percent_less_time'][v]for r in rows),2),flush=True)
