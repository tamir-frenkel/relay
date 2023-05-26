use std::alloc::GlobalAlloc;
use std::time::Duration;

pub use memoria::StatsRecorder;
use num_enum::{IntoPrimitive, TryFromPrimitive};

use crate::metric;
use crate::CounterMetric;

use memoria::{Alloc, UseCase};

pub enum AllocCounters {
    /// Tracks memory allocated and deallocated
    Alloc,
    Error,
}

impl CounterMetric for AllocCounters {
    fn name(&self) -> &'static str {
        match *self {
            AllocCounters::Alloc => "alloc",
            AllocCounters::Error => "alloc.error",
        }
    }
}

#[derive(TryFromPrimitive, IntoPrimitive, Default, Debug)]
#[repr(u32)]
pub enum RelayMemoryUseCase {
    #[default]
    None,
    ProcessEnvelope,
    ProjectCache,
    StoreEnvelope,
    TrackOutcome,
    TrackOutcomeAggregator,
    ManageEnvelope,
    GetRelay,
}

impl UseCase for RelayMemoryUseCase {}

pub type Allocator<A> = Alloc<RelayMemoryUseCase, StatsRecorder<RelayMemoryUseCase>, A>;

pub fn launch_statsd_memory_thread<A: GlobalAlloc + Send + Sync>(allocator: &'static Allocator<A>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        allocator
            .with_recorder(|recorder| {
                recorder.flush(
                    |use_case, stat| {
                        metric!(
                            counter(AllocCounters::Alloc) += stat.current as i64,
                            use_case = &format!("{:?}", use_case)
                        );
                    },
                    |err, count| {
                        metric!(
                            counter(AllocCounters::Error) += count as i64,
                            error_code = &format!("{:?}", err)
                        );
                    },
                );
                Ok(())
            })
            .ok();
    });
}
