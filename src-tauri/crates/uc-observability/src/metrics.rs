use std::sync::{Arc, LazyLock, Mutex, RwLock};
use std::time::{Duration, Instant};

use opentelemetry::metrics::{Counter, MeterProvider as _, ObservableGauge};
use opentelemetry::KeyValue;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use sysinfo::{get_current_pid, ProcessRefreshKind, ProcessesToUpdate, System};

const CLIPBOARD_OPERATIONS_METRIC: &str = "uniclipboard.clipboard.operations";
const PROCESS_CPU_UTILIZATION_METRIC: &str = "process.cpu.utilization";
const PROCESS_MEMORY_USAGE_METRIC: &str = "process.memory.usage";
const PROCESS_SAMPLE_CACHE_TTL: Duration = Duration::from_millis(250);

static METRICS_STATE: LazyLock<RwLock<Option<MetricsState>>> = LazyLock::new(|| RwLock::new(None));

struct MetricsState {
    clipboard_operations: Counter<u64>,
    _process_cpu_utilization: Option<ObservableGauge<f64>>,
    _process_memory_usage: Option<ObservableGauge<u64>>,
}

#[derive(Clone, Copy)]
struct ProcessSample {
    cpu_utilization: f64,
    memory_usage_bytes: u64,
}

struct ProcessSampler {
    system: System,
    pid: sysinfo::Pid,
    last_sample: Option<ProcessSample>,
    last_sample_at: Option<Instant>,
}

impl ProcessSampler {
    fn new() -> Option<Self> {
        let pid = get_current_pid().ok()?;
        let mut system = System::new_all();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            false,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        Some(Self {
            system,
            pid,
            last_sample: None,
            last_sample_at: None,
        })
    }

    fn sample(&mut self) -> Option<ProcessSample> {
        if let (Some(last_sample), Some(last_sample_at)) = (self.last_sample, self.last_sample_at) {
            if last_sample_at.elapsed() < PROCESS_SAMPLE_CACHE_TTL {
                return Some(last_sample);
            }
        }

        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[self.pid]),
            false,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );

        let process = self.system.process(self.pid)?;
        let cpu_count = self.system.cpus().len().max(1) as f64;
        let sample = ProcessSample {
            cpu_utilization: (process.cpu_usage() as f64 / 100.0 / cpu_count).clamp(0.0, 1.0),
            memory_usage_bytes: process.memory(),
        };

        self.last_sample = Some(sample);
        self.last_sample_at = Some(Instant::now());

        Some(sample)
    }
}

pub(crate) fn install(meter_provider: &SdkMeterProvider) {
    let meter = meter_provider.meter("uniclipboard-desktop.metrics");
    let clipboard_operations = meter
        .u64_counter(CLIPBOARD_OPERATIONS_METRIC)
        .with_description("Counts UniClipboard clipboard copy, paste, and sync operations.")
        .with_unit("{operation}")
        .build();

    let (process_cpu_utilization, process_memory_usage) =
        if let Some(process_sampler) = ProcessSampler::new().map(|s| Arc::new(Mutex::new(s))) {
            let cpu_sampler = Arc::clone(&process_sampler);
            let process_cpu_utilization = meter
                .f64_observable_gauge(PROCESS_CPU_UTILIZATION_METRIC)
                .with_description(
                    "Current process CPU utilization normalized to available logical CPUs.",
                )
                .with_unit("1")
                .with_callback(move |observer| {
                    if let Ok(mut sampler) = cpu_sampler.lock() {
                        if let Some(sample) = sampler.sample() {
                            observer.observe(sample.cpu_utilization, &[]);
                        }
                    }
                })
                .build();

            let memory_sampler = Arc::clone(&process_sampler);
            let process_memory_usage = meter
                .u64_observable_gauge(PROCESS_MEMORY_USAGE_METRIC)
                .with_description("Current process resident memory usage in bytes.")
                .with_unit("By")
                .with_callback(move |observer| {
                    if let Ok(mut sampler) = memory_sampler.lock() {
                        if let Some(sample) = sampler.sample() {
                            observer.observe(sample.memory_usage_bytes, &[]);
                        }
                    }
                })
                .build();

            (Some(process_cpu_utilization), Some(process_memory_usage))
        } else {
            (None, None)
        };

    if let Ok(mut guard) = METRICS_STATE.write() {
        *guard = Some(MetricsState {
            clipboard_operations,
            _process_cpu_utilization: process_cpu_utilization,
            _process_memory_usage: process_memory_usage,
        });
    }
}

pub(crate) fn clear() {
    if let Ok(mut guard) = METRICS_STATE.write() {
        *guard = None;
    }
}

pub fn record_clipboard_copy() {
    record_clipboard_operation(1, "copy", None);
}

pub fn record_clipboard_paste() {
    record_clipboard_operation(1, "paste", None);
}

pub fn record_clipboard_sync_inbound() {
    record_clipboard_operation(1, "sync", Some("inbound"));
}

pub fn record_clipboard_sync_outbound() {
    record_clipboard_operation(1, "sync", Some("outbound"));
}

fn record_clipboard_operation(
    value: u64,
    operation: &'static str,
    direction: Option<&'static str>,
) {
    let guard = match METRICS_STATE.read() {
        Ok(guard) => guard,
        Err(_) => return,
    };
    let Some(state) = guard.as_ref() else {
        return;
    };

    let mut attributes = Vec::with_capacity(2);
    attributes.push(KeyValue::new("operation", operation));
    if let Some(direction) = direction {
        attributes.push(KeyValue::new("direction", direction));
    }

    state.clipboard_operations.add(value, &attributes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_clipboard_operations_after_install_do_not_panic() {
        clear();

        let provider = SdkMeterProvider::builder().build();
        install(&provider);

        record_clipboard_copy();
        record_clipboard_paste();
        record_clipboard_sync_inbound();
        record_clipboard_sync_outbound();

        clear();
        provider
            .shutdown()
            .expect("meter provider should shut down");
    }
}
