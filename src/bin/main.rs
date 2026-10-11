use perfrs::events::{Hardware, L1D_ACCESS, L1D_MISS};
use perfrs::{GroupConfig, cfg_group, config, perf_group};

fn workload() -> u64 {
    let mut vec = (0..=51i32)
        .map(|x| x.div_euclid(5).isolate_highest_one().isolate_lowest_one())
        .collect::<Vec<_>>();
    vec.shrink_to_fit();
    vec.len() as u64 + vec.iter().map(|v| *v as u64).sum::<u64>()
}

fn main() -> std::io::Result<()> {
    println!("start");

    /* 
    let config = GroupConfig::new()
        .exclude_kernel(true)
        .exclude_hv(true)
        .pinned(false);
    */

    let config = cfg_group!(
        exclude_kernel => true,
        exclude_hv => true,
        pinned => true
    );

    let mut group = perf_group!(
        config;
        "cycles" => Hardware::CPU_CYCLES,
        "instructions" => Hardware::INSTRUCTIONS,
        "branches" => Hardware::BRANCH_INSTRUCTIONS,
        "branch_misses" => Hardware::BRANCH_MISSES,
        "l1d_access" => L1D_ACCESS,
        "l1d_miss" => L1D_MISS,
    )?;

    if !group.try_counter("stalled_frontend", Hardware::STALLED_CYCLES_FRONTEND)? {
        println!("stalled_frontend не поддерживается — пропущен");
    }
    if group.passes() > 1 {
        println!(
            "счётчиков больше, чем регистров: {} прогон(ов), вне основной группы: {:?}",
            group.passes(),
            group.overflow().collect::<Vec<_>>()
        );
    }

    group.counter_with("page_faults", perfrs::events::Software::PAGE_FAULTS, |b| {
        b.exclude_kernel(true);
    })?;

    // Прогрев: частота, предиктор, кэши — иначе короткий замер даёт
    // сильно заниженный IPC.
    group.warmup(workload)?;

    let (result, samples) = group.measure(workload)?;

    println!("workload returned {result}");
    print!("{samples}");

    if let Some(ipc) = samples.ratio("instructions", "cycles") {
        println!("IPC: {ipc:.2}");
    }
    if let Some(miss) = samples.percent("l1d_miss", "l1d_access") {
        println!("L1D miss rate: {miss:.1}%");
    }
    if let Some(miss) = samples.percent("branch_misses", "branches") {
        println!("branch miss rate: {miss:.1}%");
    }

    println!("\nПоддержка событий:");
    perfrs::print_check();

    Ok(())
}
