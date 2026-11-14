//! API key de Claude guardada en el llavero del sistema (Keychain / Credential Manager).
//! La clave nunca se envía al frontend.

const SERVICE: &str = "com.seru96.nova";
const ACCOUNT: &str = "anthropic-api-key";

fn entry() -> Option<keyring::Entry> {
    keyring::Entry::new(SERVICE, ACCOUNT).ok()
}

pub fn get_api_key() -> Option<String> {
    if let Ok(k) = std::env::var("NOVA_ANTHROPIC_API_KEY") {
        if !k.trim().is_empty() {
            return Some(k);
        }
    }
    entry()?.get_password().ok().filter(|k| !k.trim().is_empty())
}

pub fn has_api_key() -> bool {
    get_api_key().is_some()
}

pub fn set_api_key(key: &str) -> Result<(), String> {
    let e = entry().ok_or("no se pudo abrir el llavero del sistema")?;
    e.set_password(key.trim()).map_err(|e| e.to_string())
}

pub fn clear_api_key() -> Result<(), String> {
    let e = entry().ok_or("no se pudo abrir el llavero del sistema")?;
    match e.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
