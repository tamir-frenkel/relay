use relay_statsd::alloc::{Allocator, StatsRecorder};

#[cfg(target_os = "linux")]
type InnerAllocator = tikv_jemallocator::Jemalloc;
#[cfg(target_os = "linux")]
const INNER_ALLOCATOR: InnerAllocator = tikv_jemallocator::Jemalloc;

#[cfg(not(target_os = "linux"))]
type InnerAllocator = std::alloc::System;
#[cfg(not(target_os = "linux"))]
const INNER_ALLOCATOR: InnerAllocator = std::alloc::System;

#[global_allocator]
pub static ALLOCATOR: Allocator<InnerAllocator> =
    Allocator::new_with(StatsRecorder::new(), INNER_ALLOCATOR);
