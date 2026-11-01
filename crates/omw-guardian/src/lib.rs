//! Anti-OOM Guardian & system watchdog for Oh My Works.

use omw_core::MemoryPressure;

#[derive(Debug, PartialEq, Eq)]
pub enum HealthLevel {
    Safe,
    Warning,
    Critical,
}

pub struct Guardian;

impl Guardian {
    pub fn assess() -> Result<(HealthLevel, MemoryPressure), String> {
        let pressure = MemoryPressure::read_current()?;
        let level = if pressure.is_critical() {
            HealthLevel::Critical
        } else if pressure.some_avg10 > 10.0 || pressure.available_percent() < 15.0 {
            HealthLevel::Warning
        } else {
            HealthLevel::Safe
        };
        Ok((level, pressure))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guardian_assess() {
        let res = Guardian::assess();
        assert!(res.is_ok());
    }
}
