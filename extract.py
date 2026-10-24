from pathlib import Path
import argparse
import hashlib
import json
import shutil

parser = argparse.ArgumentParser(description="Re-extract the pinned Cargo source and rustc dep-info fixtures.")
parser.add_argument("repo", type=Path)
parser.add_argument("corpus", type=Path, help="target directory from cargo check --lib --locked")
parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parent)
args = parser.parse_args()
repo = args.repo.resolve()
corpus = args.corpus.resolve()
out = args.output.resolve()
source_path = 'src/compiler/fingerprint/dep_info.rs'
helper_path = 'crates/cargo-util/src/paths.rs'
revision = 'cf29d0e0b1f7b03c4ceda5211cc4dc493bfc542e'
src = repo / source_path
source_bytes = src.read_bytes()
helper_bytes = (repo / helper_path).read_bytes()
source_hash = hashlib.sha256(source_bytes).hexdigest()
helper_hash = hashlib.sha256(helper_bytes).hexdigest()
if source_hash != '78f239405857e94fee00e1cc6728ded70a9b805ba8193a82c161f65c562e90a1':
    parser.error("dep_info.rs differs from the pinned source")
if helper_hash != '2bc0af2a50a67facf0a5929fa4be5bc3beed97b02d5f5355a931cecb2135f96e':
    parser.error("paths.rs differs from the pinned source")
text = source_bytes.decode()
lines = text.splitlines(keepends=True)
inputs = sorted((corpus / 'debug/deps').glob('*.d'))
if not inputs:
    parser.error("no rustc dep-info files found in CORPUS/debug/deps")
fixtures = out / 'fixtures'
extra = {p.name for p in fixtures.glob('*.d')} - {p.name for p in inputs}
if extra:
    parser.error("output contains fixtures outside this corpus; use a fresh --output directory")

def section(start, end):
    return ''.join(lines[start - 1:end])

assert 'pub struct RustcDepInfo' in section(30, 47)
assert 'pub fn parse_rustc_dep_info' in section(394, 470)
assert 'pub enum InvalidChecksum' in section(703, 711)

common = '''#![allow(dead_code)]
use std::{fmt, path::PathBuf, str::{self, FromStr}};
use crate::paths;
use serde::Serialize;
type CargoResult<T> = anyhow::Result<T>;
'''
common += section(27, 28) + section(49, 267)
common += section(533, 575) + section(650, 711)
(out / 'src').mkdir(parents=True, exist_ok=True)
(out / 'src/common.rs').write_text(common)
helpers = (
    f'// Extracted from cargo-util at {revision}.\n'
    'use anyhow::Result;\n#[cfg(unix)]\nuse std::ffi::OsStr;\n'
    'use std::path::{Path, PathBuf};\n\n'
    + ''.join(helper_bytes.decode().splitlines(keepends=True)[394:429])
)
(out / 'src/paths.rs').write_text(helpers)
(out / 'capture-root.txt').write_text(str(repo))

variants = {
    'hash_fx': ('rustc_hash::FxHashMap', False),
    'btree': ('std::collections::BTreeMap', False),
    'index_fx_sort_load': ('indexmap::IndexMap<K, V, rustc_hash::FxBuildHasher>', True),
    'hash_fx_sort_write': ('rustc_hash::FxHashMap', False),
    'hash_std': ('std::collections::HashMap', False),
    'index_std_sort_load': ('indexmap::IndexMap', True),
    'hash_std_sort_write': ('std::collections::HashMap', False),
}
for name, (collection, sort) in variants.items():
    alias = (f'type HashMap<K, V> = {collection};\n' if '<K,' in collection
             else f'use {collection} as HashMap;\n')
    body = section(30, 47) + section(394, 470)
    body = body.replace('pub fn parse_rustc_dep_info(rustc_dep_info: &Path)',
                        'pub fn parse_rustc_dep_info(contents: &str)')
    body = body.replace('    let contents = paths::read(rustc_dep_info)?;\n', '')
    body = body.replace('crate::util::internal(', 'anyhow::anyhow!(')
    if sort:
        body = body.replace('    return Ok(ret);', '    ret.files.sort_unstable_keys();\n    return Ok(ret);')
    (out / f'src/{name}.rs').write_text(
        'use super::common::*;\nuse std::{path::PathBuf, str::FromStr};\n'
        'use anyhow::bail;\ntype CargoResult<T> = anyhow::Result<T>;\n' + alias + body)

fixtures.mkdir(exist_ok=True)
manifest = []
for path in inputs:
    data = path.read_bytes()
    shutil.copyfile(path, fixtures / path.name)
    manifest.append({'name': path.name, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()})
(out / 'provenance.json').write_text(json.dumps({
    'source': source_path, 'source_sha256': source_hash,
    'source_revision': revision,
    'source_url': f'https://github.com/rust-lang/cargo/blob/{revision}/{source_path}',
    'path_helpers': helper_path, 'path_helpers_sha256': helper_hash,
    'corpus_command': 'cargo check --lib --locked --target-dir TARGET_DIR',
    'captured_package_root': str(repo), 'fixtures': manifest,
    'adaptations': ['Input preloaded as &str instead of reading file',
                    'Internal malformed-input error uses anyhow directly',
                    'Collection alias and IndexMap final sort vary by case',
                    'Unused Checksum compute method omitted',
                    'Path conversion helpers extracted to remove the local cargo-util dependency'],
}, indent=2) + '\n')
print(f'Extracted seven variants and {len(manifest)} real rustc dep-info files')
