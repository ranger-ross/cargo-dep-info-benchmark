use super::*;
use std::io::Write;
use std::time::Instant;

pub fn run(fixtures: &[(String, String, usize)], source_root: Option<&Path>) {
    let cargo_index = fixtures
        .iter()
        .position(|f| f.0.starts_with("cargo-"))
        .unwrap();
    let encoded = btree::pipeline(&fixtures[cargo_index].1);
    let mut jobs = Vec::new();
    for prefix in ["cargo_credential-", "http-", "syn-", "gix-", "cargo-"] {
        let index = fixtures
            .iter()
            .position(|f| f.0.starts_with(prefix))
            .unwrap();
        for case in 0..CASES.len() {
            jobs.push((
                format!(
                    "parse_encode/{}/{}_{}",
                    prefix.trim_end_matches('-'),
                    fixtures[index].2,
                    CASES[case].0
                ),
                0,
                case,
                index,
            ));
        }
    }
    for case in 0..CASES.len() {
        jobs.push((
            format!("corpus_{}/{}", fixtures.len(), CASES[case].0),
            1,
            case,
            0,
        ));
        jobs.push((
            format!("cargo_read/{}", CASES[case].0),
            2,
            case,
            cargo_index,
        ));
        if case < 4 && source_root.is_some() {
            jobs.push((
                format!("cargo_warm/{}", CASES[case].0),
                3,
                case,
                cargo_index,
            ));
        }
    }
    let execute = |job: usize| {
        let (_, kind, case, index) = &jobs[job];
        let (_, pipeline, _, warm, read) = CASES[*case];
        match kind {
            0 => {
                black_box(pipeline(black_box(&fixtures[*index].1)));
            }
            1 => {
                for (_, input, _) in fixtures {
                    black_box(pipeline(black_box(input)));
                }
            }
            2 => {
                read(black_box(&encoded));
            }
            3 => {
                black_box(warm(black_box(&fixtures[*index].1), source_root.unwrap()));
            }
            _ => unreachable!(),
        }
    };
    let mut counts = Vec::new();
    for job in 0..jobs.len() {
        let mut count = 0;
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(5) {
            execute(job);
            count += 1;
        }
        counts.push(((count as f64 * 0.020 / start.elapsed().as_secs_f64()) as usize).max(1));
    }
    let mut samples = Vec::new();
    let mut order: Vec<_> = (0..jobs.len()).collect();
    let mut state = 0x16693_u64;
    for round in 0..30 {
        for i in (1..order.len()).rev() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            order.swap(i, state as usize % (i + 1));
        }
        for &job in &order {
            let count = counts[job];
            let start = Instant::now();
            for _ in 0..count {
                execute(job);
            }
            let ns = start.elapsed().as_nanos() as f64 / count as f64;
            samples.push((round, job, count, ns));
        }
    }
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("results/local");
    std::fs::create_dir_all(&output).unwrap();
    let mut file =
        std::io::BufWriter::new(std::fs::File::create(output.join("interleaved.csv")).unwrap());
    writeln!(file, "round,benchmark,iterations,ns_per_iteration").unwrap();
    for (round, job, count, ns) in &samples {
        writeln!(file, "{round},{},{count},{ns}", jobs[*job].0).unwrap();
    }
    file.flush().unwrap();
    for job in 0..jobs.len() {
        let mut times: Vec<_> = samples.iter().filter(|s| s.1 == job).map(|s| s.3).collect();
        times.sort_by(f64::total_cmp);
        println!(
            "{}: median {:.3} us, mean {:.3} us",
            jobs[job].0,
            (times[14] + times[15]) / 2000.0,
            times.iter().sum::<f64>() / times.len() as f64 / 1000.0
        );
    }
    println!(
        "Saved {} interleaved samples across {} cases",
        samples.len(),
        jobs.len()
    );
}
