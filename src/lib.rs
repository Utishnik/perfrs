//! Удобная обёртка над `perf_event` для группового сбора аппаратенных счётчиков.
//!
//! Базовый сценарий:
//!
//! ```no_run
//! use perfrs::{GroupConfig, PerfGroup};
//! use perfrs::events::Hardware;
//!
//! fn main() -> std::io::Result<()> {
//!     // Параметры группы задаются аргументами (chainable-сеттеры).
//!     let config = GroupConfig::new()
//!         .exclude_kernel(true)
//!         .exclude_hv(true)
//!         .one_cpu(0);
//!
//!     let mut group = PerfGroup::with_config(config)?;
//!     group.counter("cycles", Hardware::CPU_CYCLES)?;
//!     group.counter("instructions", Hardware::INSTRUCTIONS)?;
//!
//!     let (result, samples) = group.measure(|| (0..1_000_000).sum::<u64>())?;
//!     println!("{result}");
//!     println!("{samples}");
//!     println!("IPC = {:.2}", samples.ratio("instructions", "cycles").unwrap_or(f64::NAN));
//!     Ok(())
//! }
//! ```
//!
//! Макрос, если счетчиков много и лень писать `counter` по одному:
//!
//! ```no_run
//! use perfrs::{perf_group, events::Hardware};
//!
//! let mut group = perf_group! {
//!     "cycles" => Hardware::CPU_CYCLES,
//!     "instructions" => Hardware::INSTRUCTIONS,
//! }?;
//! let (_, samples) = group.measure(|| heavy_work())?;
//! # fn heavy_work() {}
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! Счётчиков больше, чем аппаратных регистров? Ничего страшного: счётчик,
//! который не влез, уходит в отдельный «шард», и [`PerfGroup::measure`]
//! сначала прогоняет код **без него** (основная группа), а потом **с ним**
//! (следующий шард). Значения сливаются в один [`Samples`], а
//! [`Samples::passes`] сообщает, сколько прогонов понадобилось.
//!
//! Учтите: прогон — это отдельный запуск кода, поэтому отношения счётчиков
//! **внутри** одного шарда точны, а **между** шардами — приблизительны.

pub mod config;
pub mod events;
pub mod group;
pub mod support;

pub use config::{GroupConfig, Observe};
pub use events::{Cache, CacheOp, CacheResult, Event, Hardware, Software, WhichCache};
pub use group::{PerfGroup, Samples};
pub use support::{Support, check, is_available, print_check};

/// Делегирование зависимостей, чтобы пользователям не нужно было
/// добавлять `perf-event` в свои `Cargo.toml`.
pub use perf_event;

/// Создаёт [`PerfGroup`] со счётчиками, перечисленными в макросе.
///
/// Возвращает `std::io::Result<PerfGroup>`, поэтому используется внутри
/// функции, возвращающей `io::Result`.
///
/// ```
/// # use perfrs::{perf_group, events::Hardware};
/// # fn demo() -> std::io::Result<()> {
/// let group = perf_group! {
///     "cycles" => Hardware::CPU_CYCLES,
///     "instructions" => Hardware::INSTRUCTIONS,
/// }?;
/// # drop(group);
/// # Ok(()) }
/// ```
///
/// Для настройки группы передайте конфигурацию отдельно:
///
/// ```
/// # use perfrs::{perf_group, GroupConfig, events::Hardware};
/// # fn demo() -> std::io::Result<()> {
/// let group = perf_group!(
///     GroupConfig::new().exclude_kernel(true);
///     "cycles" => Hardware::CPU_CYCLES,
/// )?;
/// # drop(group);
/// # Ok(()) }
/// ```
#[macro_export]
macro_rules! perf_group {
    ($config:expr; $($name:literal => $kind:expr),+ $(,)?) => {{
        (|| -> ::std::io::Result<$crate::PerfGroup> {
            let mut __perfrs_group = $crate::PerfGroup::with_config($config)?;
            $(
                __perfrs_group.counter($name, $kind)?;
            )+
            ::std::result::Result::Ok(__perfrs_group)
        })()
    }};
    ($($name:literal => $kind:expr),+ $(,)?) => {
        $crate::perf_group!($crate::GroupConfig::new(); $($name => $kind),+)
    };
}

#[macro_export]
macro_rules! cfg_group {
    ($($name:ident => $val:expr),+ $(,)?) => {
        GroupConfig::new()
        $(
            .$name($val)
        )+
    };
    () => {
        GroupConfig::new()
    };
}