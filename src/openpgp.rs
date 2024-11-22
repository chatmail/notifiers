//! Token decryption using OpenPGP.

use std::io::Cursor;

use anyhow::Result;
use base64::Engine as _;
use pgp::composed::{Deserializable as _, Message, SignedSecretKey};

/// OpenPGP message decryptor.
pub struct PgpDecryptor {
    /// Keyring of keys used for decryption.
    keyring: Vec<SignedSecretKey>,
}

impl PgpDecryptor {
    /// Creates a new OpenPGP decryptor
    /// with the given secret keys.
    pub fn new(keyring_armor: &str) -> Result<Self> {
        let cursor = Cursor::new(keyring_armor);
        let (mut secret_keys_iter, _headers) = pgp::composed::signed_key::from_armor_many(cursor)?;
        let mut secret_keys: Vec<SignedSecretKey> = Vec::new();
        for key in (&mut *secret_keys_iter).flatten() {
            if key.is_secret() {
                secret_keys.push(key.into_secret());
            }
        }
        Ok(Self {
            keyring: secret_keys,
        })
    }

    /// Decrypts incoming token from an base64-encoded OpenPGP message.
    pub fn decrypt(&self, message: &str) -> Result<String> {
        let bytes = base64::engine::general_purpose::STANDARD.decode(message)?;
        let cursor = Cursor::new(bytes);
        let msg = Message::from_bytes(cursor)?;
        let secret_key_refs: Vec<&SignedSecretKey> = self.keyring.iter().collect();
        let (msg, _key_ids) = msg.decrypt(|| "".into(), &secret_key_refs)?;
        let content = msg.get_content()?.unwrap_or_default();
        let token = String::from_utf8(content)?;

        // Remove the padding that is added
        // to avoid leaking token length.
        let token = token.trim().to_string();
        Ok(token)
    }
}
