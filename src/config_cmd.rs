use anyhow::{bail, Result};
use clap::Subcommand;

use crate::backend;

#[derive(Subcommand)]
pub enum Action {
    /// Print the effective configuration and where it lives
    Show,
    /// Set a value: `default_backend` or `claude_code.model`
    Set { key: String, value: String },
    /// Clear a value back to its default
    Unset { key: String },
}

const KEYS: &str = "default_backend, claude_code.model";

pub fn run(action: Option<Action>) -> Result<()> {
    match action.unwrap_or(Action::Show) {
        Action::Show => show(),
        Action::Set { key, value } => write(&key, Some(value)),
        Action::Unset { key } => write(&key, None),
    }
}

fn show() -> Result<()> {
    let config = backend::load_config()?;
    match backend::config_path() {
        Some(path) if path.exists() => println!("config: {}", path.display()),
        Some(path) => println!("config: {} (not created yet)", path.display()),
        None => println!("config: unavailable (HOME is not set)"),
    }
    println!(
        "  default_backend:   {}",
        config
            .default_backend
            .as_deref()
            .unwrap_or("claude-code (default)")
    );
    println!(
        "  claude_code.model: {}",
        config
            .claude_code
            .model
            .as_deref()
            .unwrap_or("(backend decides)")
    );
    if config.backends.is_empty() {
        println!("  backends:          none");
    } else {
        for (name, cfg) in &config.backends {
            println!("  backend {name}: {}", cfg.command.join(" "));
        }
    }
    println!("\nSet a value with: autos config set <key> <value>   (keys: {KEYS})");
    Ok(())
}

fn write(key: &str, value: Option<String>) -> Result<()> {
    let mut config = backend::load_config()?;
    match key {
        "default_backend" => {
            // Reject an unusable backend here rather than at the next `fix`.
            if let Some(name) = &value {
                backend::resolve(Some(name), &config)?;
            }
            config.default_backend = value.clone();
        }
        "claude_code.model" => config.claude_code.model = value.clone(),
        other => bail!("unknown config key `{other}` (known keys: {KEYS})"),
    }
    let path = backend::save_config(&config)?;
    match value {
        Some(v) => println!("{key} = {v}  ({})", path.display()),
        None => println!("{key} unset  ({})", path.display()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_key() {
        let err = write("nonexistent", Some("x".into())).unwrap_err();
        assert!(err.to_string().contains("unknown config key"));
    }
}
