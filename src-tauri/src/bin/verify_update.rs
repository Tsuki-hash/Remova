//! Release gate: validate final updater bytes against the public key embedded in the app.
use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};

fn verify() -> Result<(), Box<dyn std::error::Error>> {
    let file = std::env::args()
        .nth(1)
        .ok_or("usage: verify_update <installer>")?;
    let public = std::env::var("TAURI_UPDATER_PUBLIC_KEY")?;
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
