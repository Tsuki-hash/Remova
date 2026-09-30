//! Release gate: validate final updater bytes against the public key embedded in the app.
use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};

/// The updater key the app was BUILT with. The release pipeline injects it
/// via `.release-signing/tauri.release.json` (tauri `--config` merge); dev
/// builds have no patch file and keep the empty repo conf, where the
/// environment key alone stays authoritative. A patch file that exists but
/// cannot be read/parsed is an error — never a silent fallback.
fn expected_build_pubkey() -> Result<Option<String>, Box<dyn std::error::Error>> {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let patched = manifest.join("../.release-signing/tauri.release.json");
    if !patched.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(patched)?;
    let v: serde_json::Value = serde_json::from_str(&text)?;
    let key = v
        .get("plugins")
        .and_then(|p| p.get("updater"))
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .ok_or("patch file is missing plugins.updater.pubkey")?;
    Ok(Some(key.to_string()))
}

fn verify() -> Result<(), Box<dyn std::error::Error>> {
    let file = std::env::args()
        .nth(1)
        .ok_or("usage: verify_update <installer>")?;
    let public = std::env::var("TAURI_UPDATER_PUBLIC_KEY")?;
    // Cross-check: the verification key must be exactly the key the app was
    // built with — a drifted or mismatched env key fails the gate before any
    // signature is accepted.
    if let Some(expected) = expected_build_pubkey()? {
        if public.trim() != expected.trim() {
            return Err(
                "TAURI_UPDATER_PUBLIC_KEY does not match the updater key the app was \
                        built with (.release-signing/tauri.release.json)"
                    .into(),
            );
        }
    }
    let public = String::from_utf8(STANDARD.decode(public.trim())?)?;
    let signature = std::fs::read_to_string(format!("{file}.sig"))?;
    let signature = String::from_utf8(STANDARD.decode(signature.trim())?)?;
    PublicKey::decode(&public)?.verify(
        &std::fs::read(file)?,
        &Signature::decode(&signature)?,
        true, // Match Tauri's verifier, including non-prehashed minisign signatures.
    )?;
    println!("Updater signature verified against configured public key.");
    Ok(())
}

fn main() {
    if let Err(error) = verify() {
        eprintln!("Updater signature verification failed: {error}");
        std::process::exit(1);
    }
}
