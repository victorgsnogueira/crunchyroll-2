//! Dev tool: tries a password login and prints the raw error.
//! CR_EMAIL=... CR_PASS=... cargo run -p cr-core --example login
use crunchyroll_rs::{Crunchyroll, Locale};

#[tokio::main]
async fn main() {
    let email = std::env::var("CR_EMAIL").unwrap_or_else(|_| "nobody@example.com".into());
    let pass = std::env::var("CR_PASS").unwrap_or_else(|_| "wrong-password-123".into());
    let r = Crunchyroll::builder()
        .locale(Locale::pt_BR)
        .login_with_credentials(email, pass, Default::default())
        .await;
    match r {
        Ok(_) => println!("LOGIN OK"),
        Err(e) => println!("kind={:?}\nmsg={e}", e.kind()),
    }
}
