//! Containerized Android Emulator & E2E Test Runner bridge (Waydroid + Maestro).

use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeDriver {
    WaydroidLxc, // Native Linux container (fast, direct GPU acceleration)
    AdbBridge,   // Generic ADB daemon / WSL2 / external emulator
    Simulated,   // Fallback mock driver for headless CI
}

#[derive(Debug, Clone)]
pub struct EmulatorState {
    pub driver: RuntimeDriver,
    pub is_available: bool,
    pub device_id: String,
}

pub struct EmulatorManager;

impl EmulatorManager {
    /// Detects available Android emulation runtime on the host machine.
    /// ponytail: Probes CLI paths. Upgrade to direct Wayland socket / gRPC streaming when raw touch events and screen frames are required.
    pub fn probe_runtime() -> EmulatorState {
        // 1. Check Waydroid (Linux-native)
        #[cfg(target_os = "linux")]
        {
            if let Ok(out) = Command::new("waydroid").arg("status").output() {
                if out.status.success() {
                    return EmulatorState {
                        driver: RuntimeDriver::WaydroidLxc,
                        is_available: true,
                        device_id: "waydroid-lxc-node0".to_string(),
                    };
                }
            }
        }

        // 2. Check ADB (Cross-platform)
        if let Ok(out) = Command::new("adb").arg("devices").output() {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let devices: Vec<&str> = stdout.lines().skip(1).filter(|l| l.contains("device")).collect();
                if let Some(first) = devices.first() {
                    let dev = first.split_whitespace().next().unwrap_or("emulator-5554");
                    return EmulatorState {
                        driver: RuntimeDriver::AdbBridge,
                        is_available: true,
                        device_id: dev.to_string(),
                    };
                }
            }
        }

        // 3. Fallback simulated environment
        EmulatorState {
            driver: RuntimeDriver::Simulated,
            is_available: false,
            device_id: "simulated-android-container".to_string(),
        }
    }

    /// Generates a declarative E2E flow test plan (inspired by Maestro).
    pub fn generate_maestro_flow(app_id: &str) -> String {
        format!(
            "appId: {}\n---\n- launchApp\n- assertVisible: \"Welcome\"\n- tapOn: \"Get Started\"",
            app_id
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emulator_runtime_probe() {
        let state = EmulatorManager::probe_runtime();
        assert!(!state.device_id.is_empty());
    }

    #[test]
    fn test_maestro_flow_generation() {
        let flow = EmulatorManager::generate_maestro_flow("com.ohmyworks.mobile");
        assert!(flow.contains("appId: com.ohmyworks.mobile"));
        assert!(flow.contains("launchApp"));
    }
}
