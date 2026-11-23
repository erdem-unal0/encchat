//! Passphrase handling, on-disk paths, and encrypted local state.
//!
//! Everything here runs *before* the full-screen UI takes over the terminal,
//! because a TUI cannot prompt for a passphrase. It is shared by the TUI and
//! the `--plain` line-based client.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use encchat_core::{store, ClientCore, Psk};

/// Resolve the state and salt paths for a fingerprint.
///
/// When `explicit` is given it is treated as a base path (its extension is
/// replaced with `.state` / `.salt`); otherwise the OS data directory is used.
pub fn resolve_paths(explicit: Option<&Path>, fingerprint: &str) -> Result<(PathBuf, PathBuf)> {
    let base = match explicit {
        Some(path) => path.to_path_buf(),
        None => {
            let dir = dirs::data_dir()
                .context("could not determine a data directory")?
                .join("encchat");
            std::fs::create_dir_all(&dir)?;
            dir.join(fingerprint.replace('-', ""))
        }
    };
    if let Some(parent) = base.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok((base.with_extension("state"), base.with_extension("salt")))
}

/// Open an existing encrypted state file, or create a fresh one.
///
/// Prompts for the passphrase with `rpassword` (or reads it from stdin when
/// `password_stdin` is set). This must be called before entering raw mode.
pub fn load_or_init(
    psk: &Psk,
    state_path: &Path,
    salt_path: &Path,
    password_stdin: bool,
) -> Result<(ClientCore, store::StoreKey)> {
    if state_path.exists() && salt_path.exists() {
        let salt_bytes = std::fs::read(salt_path)?;
        let salt: store::Salt = salt_bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("local salt file is corrupt"))?;
        let password = read_password("Passphrase: ", password_stdin)?;
        let key = store::derive_key(&password, &salt)?;
        let blob = std::fs::read(state_path)?;
        let state = store::open_state(&key, &blob)
            .context("could not decrypt local state (wrong passphrase?)")?;
        Ok((ClientCore::from_state(psk.clone(), state), key))
    } else {
        let password = read_password("Choose a passphrase: ", password_stdin)?;
        let confirm = read_password("Confirm passphrase:  ", password_stdin)?;
        if password != confirm {
            bail!("passphrases do not match");
        }
        let salt = store::new_salt()?;
        let key = store::derive_key(&password, &salt)?;
        std::fs::write(salt_path, salt)?;
        let core = ClientCore::new(psk.clone())?;
        persist(&key, &core, state_path)?;
        Ok((core, key))
    }
}

fn read_password(prompt: &str, from_stdin: bool) -> Result<String> {
    if from_stdin {
        print!("{prompt}");
        std::io::stdout().flush()?;
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        Ok(line.trim_end().to_string())
    } else {
        Ok(rpassword::prompt_password(prompt)?)
    }
}

/// Encrypt and atomically write the whole local state.
pub fn persist(key: &store::StoreKey, core: &ClientCore, state_path: &Path) -> Result<()> {
    let blob = store::seal_state(key, &core.state)?;
    let tmp = state_path.with_extension("tmp");
    std::fs::write(&tmp, &blob)?;
    std::fs::rename(&tmp, state_path)?;
    Ok(())
}

/// Milliseconds since the Unix epoch, or 0 if the clock is before it.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Lower-case hex encoding.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// First four bytes of a device ID, for compact display (8 hex chars).
pub fn short(id: &[u8]) -> String {
    hex(&id[..id.len().min(4)])
}
