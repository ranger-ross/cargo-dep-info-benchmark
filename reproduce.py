from pathlib import Path
import hashlib
import json
import shutil
import subprocess
import tempfile

workspace = Path(__file__).resolve().parent
cargo = subprocess.check_output(['cargo', '--version'], text=True).strip()
records = []
with tempfile.TemporaryDirectory(prefix='cargo-dep-info-repro-') as temp:
    root = Path(temp) / 'repro'
    shutil.copytree(workspace / 'repro/src', root / 'src')
    shutil.copyfile(workspace / 'repro/Cargo.toml', root / 'Cargo.toml')
    shutil.copyfile(workspace / 'repro/Cargo.lock', root / 'Cargo.lock')
    for run in range(8):
        target = root / 'target'
        subprocess.run(['cargo', 'build', '--locked', '--manifest-path', str(root / 'Cargo.toml'),
                        '--target-dir', str(target)], cwd=root, check=True)
        dep = next((target / 'debug/.fingerprint').glob('*/dep-lib-*')).read_bytes()
        cache = (target / '.rustc_info.json').read_bytes()
        records.append({'run': run, 'dep_sha256': hashlib.sha256(dep).hexdigest(),
                        'cache_sha256': hashlib.sha256(cache).hexdigest(),
                        'cache': json.loads(cache)})
        target.rename(root / f'target-{run}')
output = workspace / 'results/local'
output.mkdir(parents=True, exist_ok=True)
(output / 'reproduction.json').write_text(json.dumps({'cargo': cargo, 'runs': records}, indent=2) + '\n')
print(cargo)
print('Clean builds:', len(records))
print('Distinct fingerprint dep-info:', len({r['dep_sha256'] for r in records}))
print('Distinct rustc cache bytes:', len({r['cache_sha256'] for r in records}))
print('Distinct rustc cache logical contents:', len({json.dumps(r['cache'], sort_keys=True) for r in records}))
print('Cache map sizes:', {k: len(v) for k, v in records[0]['cache'].items() if isinstance(v, dict)})
