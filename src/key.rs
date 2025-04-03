use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::process::Command;

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Key {
    Env { env: String },
    Cmd { cmd: String },
}

impl Key {
    pub fn get_value(&self) -> Result<String> {
        match self {
            Self::Env { env } => {
                std::env::var(env).map_err(|_| anyhow!("Environment variable '{}' not found", env))
            }
            Self::Cmd { cmd } => {
                let output = Command::new("sh")
                    .arg("-c")
                    .arg(cmd)
                    .output()
                    .map_err(|e| anyhow!("Failed to execute command '{}': {}", cmd, e))?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(anyhow!("Command '{}' failed: {}", cmd, stderr));
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                Ok(stdout.trim().to_string())
            }
        }
    }
}
