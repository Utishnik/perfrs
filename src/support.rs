//! Проверка поддержки событий на данной машине.

use std::fmt;

use perf_event::Builder;
use perf_event::events::{Event, Hardware};

/// Результат проверки одного события.
#[derive(Debug, Clone)]
pub struct Support {
    /// Отображаемое имя события.
    pub name: String,
    /// Удалось ли создать счётчик.
    pub supported: bool,
    /// Причина отказа, если событие не поддерживается.
    pub error: Option<String>,
}

impl fmt::Display for Support {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.supported {
            write!(f, "{:<32} yes", self.name)
        } else {
            match &self.error {
                Some(e) => write!(f, "{:<32} no  ({e})", self.name),
                None => write!(f, "{:<32} no", self.name),
            }
        }
    }
}

/// Проверить, доступно ли событие `kind` на этой машине.
///
/// ```
/// use perfrs::{is_available, events::Hardware};
///
/// let _ = is_available(Hardware::CPU_CYCLES);
/// ```
pub fn is_available<K: Into<Event>>(kind: K) -> bool {
    Builder::new().kind(kind).build().is_ok()
}

/// Проверить набор распространённых hardware/software-событий.
pub fn check() -> Vec<Support> {
    use perf_event::events::Software;

    let candidates: [(&str, Event); 14] = [
        ("CPU_CYCLES", Hardware::CPU_CYCLES.into()),
        ("INSTRUCTIONS", Hardware::INSTRUCTIONS.into()),
        ("BRANCH_INSTRUCTIONS", Hardware::BRANCH_INSTRUCTIONS.into()),
        ("BRANCH_MISSES", Hardware::BRANCH_MISSES.into()),
        ("CACHE_REFERENCES", Hardware::CACHE_REFERENCES.into()),
        ("CACHE_MISSES", Hardware::CACHE_MISSES.into()),
        ("BUS_CYCLES", Hardware::BUS_CYCLES.into()),
        ("REF_CPU_CYCLES", Hardware::REF_CPU_CYCLES.into()),
        (
            "STALLED_CYCLES_FRONTEND",
            Hardware::STALLED_CYCLES_FRONTEND.into(),
        ),
        (
            "STALLED_CYCLES_BACKEND",
            Hardware::STALLED_CYCLES_BACKEND.into(),
        ),
        ("SW_PAGE_FAULTS", Software::PAGE_FAULTS.into()),
        ("SW_CONTEXT_SWITCHES", Software::CONTEXT_SWITCHES.into()),
        ("SW_CPU_MIGRATIONS", Software::CPU_MIGRATIONS.into()),
        ("SW_TASK_CLOCK", Software::TASK_CLOCK.into()),
    ];

    candidates
        .into_iter()
        .map(|(name, kind)| match Builder::new().kind(kind).build() {
            Ok(_) => Support {
                name: name.to_string(),
                supported: true,
                error: None,
            },
            Err(e) => Support {
                name: name.to_string(),
                supported: false,
                error: Some(e.to_string()),
            },
        })
        .collect()
}

/// Напечатать таблицу поддержки событий (как [`check`], но сразу в stdout).
pub fn print_check() {
    println!("{:<32} Supported?", "Event");
    println!("{}", "-".repeat(45));
    for support in check() {
        println!("{support}");
    }
}
