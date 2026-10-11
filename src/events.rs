//! Переиспользуемые события и удобные конструкторы.

pub use perf_event::events::{
    Breakpoint, Cache, CacheOp, CacheResult, Event, Hardware, Software, WhichCache,
};

/// Событие кэша: `which` + `operation` + `result`.
pub const fn cache(which: WhichCache, operation: CacheOp, result: CacheResult) -> Cache {
    Cache {
        which,
        operation,
        result,
    }
}

/// Все обращения к кэшу (для пары «обращения / промахи»).
pub const fn accesses(which: WhichCache, operation: CacheOp) -> Cache {
    cache(which, operation, CacheResult::ACCESS)
}

/// Только промахи кэша.
pub const fn misses(which: WhichCache, operation: CacheOp) -> Cache {
    cache(which, operation, CacheResult::MISS)
}

/// Обращения к L1D.
pub const L1D_ACCESS: Cache = accesses(WhichCache::L1D, CacheOp::READ);
/// Промахи L1D.
pub const L1D_MISS: Cache = misses(WhichCache::L1D, CacheOp::READ);
/// Обращения к последнего уровня кэша.
pub const LL_ACCESS: Cache = accesses(WhichCache::LL, CacheOp::READ);
/// Промахи последнего уровня кэша.
pub const LL_MISS: Cache = misses(WhichCache::LL, CacheOp::READ);
/// Обращения к ITLB.
pub const ITLB_ACCESS: Cache = accesses(WhichCache::ITLB, CacheOp::READ);
/// Промахи ITLB.
pub const ITLB_MISS: Cache = misses(WhichCache::ITLB, CacheOp::READ);
/// Обращения к DTLB.
pub const DTLB_ACCESS: Cache = accesses(WhichCache::DTLB, CacheOp::READ);
/// Промахи DTLB.
pub const DTLB_MISS: Cache = misses(WhichCache::DTLB, CacheOp::READ);
