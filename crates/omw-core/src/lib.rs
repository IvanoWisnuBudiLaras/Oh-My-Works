//! Core domain models and cross-platform abstractions for Oh My Works.

use std::fs;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MemoryPressure {
    pub some_avg10: f64,
    pub some_avg60: f64,
    pub full_avg10: f64,
    pub full_avg60: f64,
    pub total_mem_kb: u64,
    pub available_mem_kb: u64,
}

impl MemoryPressure {
    /// Reads kernel memory pressure information.
    /// ponytail: Linux reads /proc/pressure/memory and /proc/meminfo directly. Add Windows Win32 API memory queries when Windows target is compiled.
    pub fn read_current() -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        {
            Self::read_linux()
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(Self::default())
        }
    }

    #[cfg(target_os = "linux")]
    fn read_linux() -> Result<Self, String> {
        let mut pressure = Self::default();

        if let Ok(psi_raw) = fs::read_to_string("/proc/pressure/memory") {
            for line in psi_raw.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.is_empty() {
                    continue;
                }
                let is_some = parts[0] == "some";
                let is_full = parts[0] == "full";

                for part in &parts[1..] {
                    if let Some((k, v)) = part.split_once('=') {
                        let val = v.parse::<f64>().unwrap_or(0.0);
                        match (is_some, is_full, k) {
                            (true, _, "avg10") => pressure.some_avg10 = val,
                            (true, _, "avg60") => pressure.some_avg60 = val,
                            (_, true, "avg10") => pressure.full_avg10 = val,
                            (_, true, "avg60") => pressure.full_avg60 = val,
                            _ => {}
                        }
                    }
                }
            }
        }

        if let Ok(mem_raw) = fs::read_to_string("/proc/meminfo") {
            for line in mem_raw.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let key = parts[0].trim_end_matches(':');
                    let val = parts[1].parse::<u64>().unwrap_or(0);
                    match key {
                        "MemTotal" => pressure.total_mem_kb = val,
                        "MemAvailable" => pressure.available_mem_kb = val,
                        _ => {}
                    }
                }
            }
        }

        Ok(pressure)
    }

    pub fn available_percent(&self) -> f64 {
        if self.total_mem_kb == 0 {
            return 100.0;
        }
        (self.available_mem_kb as f64 / self.total_mem_kb as f64) * 100.0
    }

    pub fn is_critical(&self) -> bool {
        // Critical if full memory stall > 20% or available < 5%
        self.full_avg10 > 20.0 || (self.total_mem_kb > 0 && self.available_percent() < 5.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_pressure_calculation() {
        let p = MemoryPressure {
            some_avg10: 1.5,
            some_avg60: 0.8,
            full_avg10: 0.0,
            full_avg60: 0.0,
            total_mem_kb: 10_000_000,
            available_mem_kb: 5_000_000,
        };
        assert!((p.available_percent() - 50.0).abs() < 0.01);
        assert!(!p.is_critical());

        let critical_p = MemoryPressure {
            full_avg10: 25.0,
            ..p
        };
        assert!(critical_p.is_critical());
    }
}
