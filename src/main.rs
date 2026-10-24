use criterion::{BenchmarkId, Criterion};
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Duration;

mod common;
mod interleaved;
mod paths;
use common::{Checksum, DepInfoPathType, EncodedDepInfo};

const CAPTURED_PACKAGE_ROOT: &str = include_str!("../capture-root.txt");

fn encode(
    files: impl IntoIterator<Item = (PathBuf, Option<(u64, Checksum)>)>,
    env: Vec<(String, Option<String>)>,
    sort: bool,
    source_root: Option<&Path>,
) -> Vec<u8> {
    let mut info = EncodedDepInfo {
        files: Vec::new(),
        env,
    };
    for (path, checksum) in files {
        let path = if let Some(root) = source_root {
            let canonical = std::fs::canonicalize(root.join(path))
                .expect("source files must remain available during the warm-path benchmark");
            canonical
                .strip_prefix(root)
                .unwrap_or(&canonical)
                .to_owned()
        } else {
            path
        };
        info.files.push((
            DepInfoPathType::PackageRootRelative,
            path,
            checksum.map(|(len, checksum)| (len, checksum.to_string())),
        ));
    }
    if sort {
        info.files.sort_unstable_by(|a, b| a.1.cmp(&b.1));
    }
    info.serialize().unwrap()
}

macro_rules! variant {
    ($name:ident, $sort:expr) => {
        mod $name {
            include!(concat!(stringify!($name), ".rs"));
            pub fn pipeline(input: &str) -> Vec<u8> {
                let info = parse_rustc_dep_info(input).unwrap();
                super::encode(info.files, info.env, $sort, None)
            }
            pub fn warm_paths(input: &str, root: &std::path::Path) -> Vec<u8> {
                let info = parse_rustc_dep_info(input).unwrap();
                super::encode(info.files, info.env, $sort, Some(root))
            }
            pub fn parse_only(input: &str) {
                std::hint::black_box(parse_rustc_dep_info(input).unwrap());
            }
            pub fn read_encoded(input: &[u8]) {
                let info = EncodedDepInfo::parse(input).unwrap();
                let mut ret = RustcDepInfo::default();
                ret.env = info.env;
                ret.files
                    .extend(info.files.into_iter().map(|(_, path, checksum)| {
                        (
                            std::path::Path::new(super::CAPTURED_PACKAGE_ROOT).join(path),
                            checksum.and_then(|(len, checksum)| {
                                checksum.parse().ok().map(|c| (len, c))
                            }),
                        )
                    }));
                std::hint::black_box(ret);
            }
        }
    };
}
variant!(hash_fx, false);
variant!(btree, false);
variant!(index_fx_sort_load, false);
variant!(hash_fx_sort_write, true);
variant!(hash_std, false);
variant!(index_std_sort_load, false);
variant!(hash_std_sort_write, true);

type Pipeline = fn(&str) -> Vec<u8>;
type WarmPipeline = fn(&str, &Path) -> Vec<u8>;
const CASES: [(&str, Pipeline, fn(&str), WarmPipeline, fn(&[u8])); 7] = [
    (
        "hash_fx",
        hash_fx::pipeline,
        hash_fx::parse_only,
        hash_fx::warm_paths,
        hash_fx::read_encoded,
    ),
    (
        "btree",
        btree::pipeline,
        btree::parse_only,
        btree::warm_paths,
        btree::read_encoded,
    ),
    (
        "index_fx_sort_load",
        index_fx_sort_load::pipeline,
        index_fx_sort_load::parse_only,
        index_fx_sort_load::warm_paths,
        index_fx_sort_load::read_encoded,
    ),
    (
        "hash_fx_sort_write",
        hash_fx_sort_write::pipeline,
        hash_fx_sort_write::parse_only,
        hash_fx_sort_write::warm_paths,
        hash_fx_sort_write::read_encoded,
    ),
    (
        "hash_std",
        hash_std::pipeline,
        hash_std::parse_only,
        hash_std::warm_paths,
        hash_std::read_encoded,
    ),
    (
        "index_std_sort_load",
        index_std_sort_load::pipeline,
        index_std_sort_load::parse_only,
        index_std_sort_load::warm_paths,
        index_std_sort_load::read_encoded,
    ),
    (
        "hash_std_sort_write",
        hash_std_sort_write::pipeline,
        hash_std_sort_write::parse_only,
        hash_std_sort_write::warm_paths,
        hash_std_sort_write::read_encoded,
    ),
];

fn normalized(bytes: &[u8]) -> Vec<u8> {
    let mut info = EncodedDepInfo::parse(bytes).unwrap();
    info.files.sort_unstable_by(|a, b| a.1.cmp(&b.1));
    info.serialize().unwrap()
}

fn verify(input: &str) {
    let expected = btree::pipeline(input);
    for (name, pipeline, _, _, _) in CASES {
        let output = pipeline(input);
        assert_eq!(normalized(&output), expected, "semantic mismatch: {name}");
        if name != "hash_fx" && name != "hash_std" {
            assert_eq!(output, expected, "byte mismatch: {name}");
        }
    }
}

fn main() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut fixtures: Vec<_> = std::fs::read_dir(fixture_dir)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let input = std::fs::read_to_string(&path).unwrap();
            let parsed = hash_fx::parse_rustc_dep_info(&input).unwrap();
            (
                path.file_name().unwrap().to_str().unwrap().to_owned(),
                input,
                parsed.files.len(),
            )
        })
        .collect();
    fixtures.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    let source_root = std::env::var_os("DEPINFO_SOURCE_ROOT").map(|root| {
        let root = std::fs::canonicalize(root).expect("DEPINFO_SOURCE_ROOT must exist");
        let cargo = &fixtures
            .iter()
            .find(|f| f.0.starts_with("cargo-"))
            .unwrap()
            .1;
        for path in hash_fx::parse_rustc_dep_info(cargo).unwrap().files.keys() {
            assert!(
                path.is_relative(),
                "warm-path fixture contains an absolute dependency"
            );
            assert!(
                root.join(path).is_file(),
                "missing source file: {}",
                root.join(path).display()
            );
        }
        root
    });
    for (_, input, _) in &fixtures {
        verify(input);
        let mut lines: Vec<String> = input.lines().map(str::to_owned).collect();
        let line = lines.iter_mut().find(|line| line.contains(": ")).unwrap();
        let (target, deps) = line.split_once(": ").unwrap();
        assert!(!deps.contains("\\ "), "shuffle needs escaped-path handling");
        let mut deps: Vec<_> = deps.split_whitespace().collect();
        deps.reverse();
        *line = format!("{target}: {}", deps.join(" "));
        let reordered = lines.join("\n");
        verify(&reordered);
        assert_eq!(btree::pipeline(input), btree::pipeline(&reordered));
    }
    let edge = "out: src/z.rs src/a\\ b.rs src/z.rs\n\n# env-dep:SET=a\\nb\\\\c\n# env-dep:UNSET\n# checksum:sha256=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef file_len:42 src/z.rs\n";
    verify(edge);
    let info = EncodedDepInfo::parse(&btree::pipeline(edge)).unwrap();
    assert_eq!(info.files.len(), 2);
    assert_eq!(info.files[0].1, PathBuf::from("src/a b.rs"));
    assert_eq!(info.files[1].2.as_ref().unwrap().0, 42);
    assert_eq!(
        info.env,
        vec![
            ("SET".into(), Some("a\nb\\c".into())),
            ("UNSET".into(), None)
        ]
    );
    let order_a = "out: src/z.rs src/a.rs src/b.rs src/c.rs src/d.rs src/e.rs\n";
    let order_b = "out: src/e.rs src/d.rs src/c.rs src/b.rs src/a.rs src/z.rs\n";
    let std_unique = (0..32)
        .map(|_| hash_std::pipeline(order_a))
        .collect::<std::collections::HashSet<_>>()
        .len();
    let fx_changed = fixtures
        .iter()
        .filter(|(_, input, _)| {
            let mut lines: Vec<_> = input.lines().map(str::to_owned).collect();
            let line = lines.iter_mut().find(|line| line.contains(": ")).unwrap();
            let (target, deps) = line.split_once(": ").unwrap();
            *line = format!(
                "{target}: {}",
                deps.split_whitespace().rev().collect::<Vec<_>>().join(" ")
            );
            hash_fx::pipeline(input) != hash_fx::pipeline(&lines.join("\n"))
        })
        .count();
    assert_eq!(btree::pipeline(order_a), btree::pipeline(order_b));
    println!(
        "Verified {} fixtures, reversed insertion order, escaped paths, duplicate paths, env values, and checksum updates.",
        fixtures.len()
    );
    println!(
        "Std HashMap: {std_unique} distinct encodings from 32 identical inputs. Fx HashMap: {fx_changed} fixtures changed bytes after reversing dependency order."
    );
    for prefix in ["cargo_credential-", "http-", "syn-", "gix-", "cargo-"] {
        let (name, input, count) = fixtures.iter().find(|f| f.0.starts_with(prefix)).unwrap();
        let parsed = hash_fx::parse_rustc_dep_info(input).unwrap();
        println!(
            "Fixture {name}: {count} paths, {} bytes, {} env entries",
            input.len(),
            parsed.env.len()
        );
    }
    if std::env::args().any(|arg| arg == "--verify") {
        return;
    }
    if std::env::args().any(|arg| arg == "--interleaved") {
        interleaved::run(&fixtures, source_root.as_deref());
        return;
    }

    let mut criterion = Criterion::default()
        .output_directory(&Path::new(env!("CARGO_MANIFEST_DIR")).join("target/criterion"))
        .sample_size(50)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2))
        .configure_from_args();
    let mut group = criterion.benchmark_group("parse_encode");
    for prefix in ["cargo_credential-", "http-", "syn-", "gix-", "cargo-"] {
        let (_, input, count) = fixtures.iter().find(|f| f.0.starts_with(prefix)).unwrap();
        for (name, pipeline, _, _, _) in CASES {
            group.bench_with_input(
                BenchmarkId::new(prefix.trim_end_matches('-'), format!("{count}_{name}")),
                input,
                |b, input| {
                    b.iter(|| black_box(pipeline(black_box(input))));
                },
            );
        }
    }
    group.finish();
    let cargo = &fixtures
        .iter()
        .find(|f| f.0.starts_with("cargo-"))
        .unwrap()
        .1;
    let encoded = btree::pipeline(cargo);
    let mut group = criterion.benchmark_group("cargo_stages");
    for (name, _, parse, warm, read) in CASES {
        group.bench_function(format!("parse/{name}"), |b| {
            b.iter(|| parse(black_box(cargo)))
        });
        group.bench_function(format!("read_encoded/{name}"), |b| {
            b.iter(|| read(black_box(&encoded)))
        });
        if let Some(root) = source_root.as_deref()
            && [
                "hash_fx",
                "btree",
                "index_fx_sort_load",
                "hash_fx_sort_write",
            ]
            .contains(&name)
        {
            group.bench_function(format!("warm_canonicalize_encode/{name}"), |b| {
                b.iter(|| black_box(warm(black_box(cargo), root)))
            });
        }
    }
    group.finish();
    let mut group = criterion.benchmark_group(format!("corpus_{}", fixtures.len()));
    for (name, pipeline, _, _, _) in CASES {
        group.bench_function(name, |b| {
            b.iter(|| {
                for (_, input, _) in &fixtures {
                    black_box(pipeline(black_box(input)));
                }
            })
        });
    }
    group.finish();
    criterion.final_summary();
}
