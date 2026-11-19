//! Secret storage backed by the macOS Keychain.
//!
//! Values never travel through a shell: `security` is invoked with an argument
//! vector. A workspace stores only a `keychain://<account>` reference, so the
//! JSON can be committed or shared without leaking anything.
//!
//! Tests use an in-memory backend so the logic is verified without touching the
//! real Keychain.

const PREFIX: &str = "keychain://";

pub fn reference(account: &str) -> String {
    format!("{PREFIX}{account}")
}

pub fn is_reference(value: &str) -> bool {
    value.starts_with(PREFIX)
}

pub fn account_of(value: &str) -> Option<&str> {
    value.strip_prefix(PREFIX).filter(|rest| !rest.is_empty())
}

/// Whether the platform can store secrets. The real check happens on write.
pub fn available() -> bool {
    cfg!(target_os = "macos")
}

pub fn store(account: &str, secret: &str) -> Result<(), String> {
    if !available() {
        return Err("The Keychain is only available on macOS.".to_string());
    }
    backend::put(account, secret)
}

/// Reads a secret, `Ok(None)` when it was never stored.
pub fn load(account: &str) -> Result<Option<String>, String> {
    if !available() {
        return Ok(None);
    }
    backend::get(account)
}

pub fn delete(account: &str) -> Result<(), String> {
    if !available() {
        return Ok(());
    }
    backend::remove(account)
}

/// End-to-end check used by Settings: store, read back and remove a probe item.
pub fn self_test() -> Result<(), String> {
    let account = "probe";
    store(account, "lucapi-probe")?;
    let read = load(account)?;
    let _ = delete(account);
    match read {
        Some(value) if value == "lucapi-probe" => Ok(()),
        _ => Err("The Keychain did not return the value it stored.".to_string()),
    }
}

/// Reads a value that may be a `keychain://` reference.
pub fn resolve(value: &str) -> Option<String> {
    let account = account_of(value)?;
    load(account).ok().flatten()
}

#[cfg(not(test))]
mod backend {
    use std::process::Command;

    const SERVICE: &str = "dev.lucapi.secrets";

    pub fn put(account: &str, secret: &str) -> Result<(), String> {
        let output = Command::new("security")
            .args([
                "add-generic-password",
                "-a",
                account,
                "-s",
                SERVICE,
                "-w",
                secret,
                "-U",
            ])
            .output()
            .map_err(|error| format!("Could not run the Keychain tool: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }

    pub fn get(account: &str) -> Result<Option<String>, String> {
        let output = Command::new("security")
            .args(["find-generic-password", "-a", account, "-s", SERVICE, "-w"])
            .output()
            .map_err(|error| format!("Could not run the Keychain tool: {error}"))?;
        if output.status.success() {
            let value = String::from_utf8_lossy(&output.stdout);
            Ok(Some(value.trim_end_matches(['\n', '\r']).to_string()))
        } else {
            Ok(None)
        }
    }

    pub fn remove(account: &str) -> Result<(), String> {
        let _ = Command::new("security")
            .args(["delete-generic-password", "-a", account, "-s", SERVICE])
            .output();
        Ok(())
    }
}

#[cfg(test)]
mod backend {
    use std::sync::Mutex;

    static ITEMS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

    pub fn put(account: &str, secret: &str) -> Result<(), String> {
        let mut items = ITEMS.lock().map_err(|_| "vault lock".to_string())?;
        match items.iter_mut().find(|(key, _)| key == account) {
            Some(slot) => slot.1 = secret.to_string(),
            None => items.push((account.to_string(), secret.to_string())),
        }
        Ok(())
    }

    pub fn get(account: &str) -> Result<Option<String>, String> {
        let items = ITEMS.lock().map_err(|_| "vault lock".to_string())?;
        Ok(items
            .iter()
            .find(|(key, _)| key == account)
            .map(|(_, value)| value.clone()))
    }

    pub fn remove(account: &str) -> Result<(), String> {
        let mut items = ITEMS.lock().map_err(|_| "vault lock".to_string())?;
        items.retain(|(key, _)| key != account);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_round_trip() {
        let reference = reference("env:12:token");
        assert!(is_reference(&reference));
        assert_eq!(account_of(&reference), Some("env:12:token"));
        assert_eq!(account_of("plain"), None);
        assert_eq!(account_of("keychain://"), None);
    }

    #[test]
    fn store_load_delete() {
        store("test:account", "value-1").unwrap();
        assert_eq!(load("test:account").unwrap().as_deref(), Some("value-1"));
        store("test:account", "value-2").unwrap();
        assert_eq!(load("test:account").unwrap().as_deref(), Some("value-2"));
        delete("test:account").unwrap();
        assert_eq!(load("test:account").unwrap(), None);
    }

    #[test]
    fn resolve_reads_through_references() {
        store("auth:req:bearer", "token-123").unwrap();
        assert_eq!(
            resolve("keychain://auth:req:bearer").as_deref(),
            Some("token-123")
        );
        assert_eq!(resolve("plain-value"), None);
    }
}
