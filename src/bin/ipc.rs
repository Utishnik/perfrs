use perfrs::events::Hardware;
use perfrs::{GroupConfig, PerfGroup};
use std::hint::black_box;

fn workload(iters: u32) -> u64 {
    let mut acc = 0u64;
    for _ in 0..iters {
        let mut v = (0..=51i32)
            .map(|x| x.div_euclid(5).isolate_highest_one().isolate_lowest_one())
            .collect::<Vec<_>>();
        v.shrink_to_fit();
        acc += v.len() as u64 + v.iter().map(|x| *x as u64).sum::<u64>();
        black_box(&mut v);
    }
    acc
}

fn tight_loop(iters: u64) -> u64 {
    let mut acc = 0u64;
    for i in 0..iters {
        acc = acc.wrapping_mul(31).wrapping_add(i);
        acc ^= acc >> 13;
    }
    black_box(acc)
}

fn report(label: &str, samples: &perfrs::Samples) {
    let cyc = samples["cycles"] as f64;
    let insn = samples["instructions"] as f64;
    let ghz = cyc / samples.time_enabled().max(1) as f64;
    let b = samples.get("branches").unwrap_or(0) as f64;
    let bm = samples.get("branch_misses").unwrap_or(0) as f64;
    println!(
        "{label:<28} ipc={:>5.2} cpi={:>5.2} cyc={:>9} insn={:>8} \
         time={:>8}ns f≈{:.2}GHz duty={:.2} bmiss={:>5.1}%",
        insn / cyc,
        cyc / insn,
        samples["cycles"],
        samples["instructions"],
        samples.time_enabled(),
        ghz,
        samples.duty_cycle(),
        if b > 0.0 { bm / b * 100.0 } else { 0.0 },
    );
}

fn main() -> std::io::Result<()> {
    let config = GroupConfig::new();
    let mut g = PerfGroup::with_config(config)?;
    g.counter("cycles", Hardware::CPU_CYCLES)?;
    g.counter("instructions", Hardware::INSTRUCTIONS)?;
    g.counter("branches", Hardware::BRANCH_INSTRUCTIONS)?;
    g.counter("branch_misses", Hardware::BRANCH_MISSES)?;

    // прогрев: код, аллокатор, частота
    let _ = g.measure(|| workload(50))?;
    let _ = g.measure(|| tight_loop(1_000_000))?;

    for iters in [1u32, 5, 50, 500, 5000] {
        let (_, s) = g.measure(|| workload(iters))?;
        report(&format!("workload x{iters}"), &s);
    }

    println!();
    for iters in [10_000u64, 100_000, 1_000_000, 10_000_000] {
        let (_, s) = g.measure(|| tight_loop(iters))?;
        report(&format!("tight_loop {iters:>9}"), &s);
    }

    Ok(())
}
