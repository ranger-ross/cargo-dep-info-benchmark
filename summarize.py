from pathlib import Path
import argparse
import csv
import json
import statistics

root = Path(__file__).resolve().parent
parser = argparse.ArgumentParser(description='Summarize Criterion estimates or interleaved samples.')
parser.add_argument('baseline', nargs='?', default='local')
parser.add_argument('--criterion-root', type=Path, default=root / 'target/criterion')
parser.add_argument('--interleaved', type=Path, help='CSV produced by --interleaved')
args = parser.parse_args()
if Path(args.baseline).name != args.baseline or args.baseline in {'.', '..'}:
    parser.error('baseline must be a directory name, not a path')
rows = []
if args.interleaved:
    samples = {}
    with args.interleaved.open(newline='') as source:
        for row in csv.DictReader(source):
            samples.setdefault(row['benchmark'], []).append(float(row['ns_per_iteration']))
    for name, values in sorted(samples.items()):
        rows.append({'benchmark': name, 'samples': len(values),
                     'mean_ns': statistics.mean(values), 'median_ns': statistics.median(values)})
    filename = 'interleaved-summary.json'
else:
    for path in sorted(args.criterion_root.rglob(f'{args.baseline}/estimates.json')):
        stats = json.loads(path.read_text())
        mean = stats['mean']
        rows.append({'benchmark': str(path.parent.parent.relative_to(args.criterion_root)),
                     'mean_ns': mean['point_estimate'],
                     'mean_ci95_low_ns': mean['confidence_interval']['lower_bound'],
                     'mean_ci95_high_ns': mean['confidence_interval']['upper_bound'],
                     'median_ns': stats['median']['point_estimate']})
    filename = f'{args.baseline}-summary.json'
if not rows:
    parser.error('no benchmark results found')
output = root / 'results/local'
output.mkdir(parents=True, exist_ok=True)
(output / filename).write_text(json.dumps(rows, indent=2) + '\n')
print(f"{'Benchmark':70} {'Mean (us)':>12} {'Median (us)':>12}")
for row in rows:
    print(f"{row['benchmark']:70} {row['mean_ns']/1000:12.3f} {row['median_ns']/1000:12.3f}")
print(f'{len(rows)} estimates exported to {output / filename}')
