use std::collections::HashMap;

use crate::{
    crypto::{
        aead::AeadDecrypt,
        cipher_suite::{CipherSuite, CipherSuiteVariant},
        key_expansion::{KeyMaterial, Secret},
    },
    error::{Result, SframeError},
    frame_validation::{FrameValidation, ReplayAttackProtection},
    header::{Deserialization, Header, HeaderFields, KeyId},
};

pub struct ReceiverOptions {
    cipher_suite: CipherSuite,
    frame_validation: Box<dyn FrameValidation>,
}

impl Default for ReceiverOptions {
    fn default() -> Self {
        Self {
            cipher_suite: CipherSuiteVariant::AesGcm256Sha512.into(),
            frame_validation: Box::new(ReplayAttackProtection::with_tolerance(128)),
        }
    }
}

pub struct Receiver {
    secrets: HashMap<KeyId, Secret>,
    options: ReceiverOptions,
}

impl Default for Receiver {
    fn default() -> Self {
        Receiver {
            secrets: Default::default(),
            options: ReceiverOptions::default(),
        }
    }
}

impl Receiver {
    pub fn decrypt(&self, encrypted_frame: &[u8], skip: usize) -> Result<Vec<u8>> {
        let header = Header::deserialize(&encrypted_frame[skip..])?;

        self.options.frame_validation.validate(&header)?;

        let key_id = header.get_key_id();
        if let Some(secret) = self.secrets.get(&key_id) {
            log::trace!(
                "Receiver: Frame counter: {:?}, Key id: {:?}",
                header.get_frame_counter(),
                header.get_key_id()
            );

            let payload_begin_idx = skip + header.size();
            let mut io_buffer: Vec<u8> = encrypted_frame[..skip]
                .iter()
                .chain(encrypted_frame[payload_begin_idx..].iter())
                .copied()
                .collect();

            self.options.cipher_suite.decrypt(
                &mut io_buffer[skip..],
                secret,
                &encrypted_frame[skip..payload_begin_idx],
                &header.get_frame_counter(),
            )?;

            io_buffer.truncate(io_buffer.len() - self.options.cipher_suite.auth_tag_len);
            Ok(io_buffer)
        } else {
            Err(SframeError::MissingDecryptionKey(key_id))
        }
    }

    // TODO: use KeyId instead of u64
    pub fn set_encryption_key(&mut self, receiver_id: u64, key_material: &[u8]) -> Result<()> {
        self.secrets.insert(
            KeyId::from(receiver_id),
            KeyMaterial(key_material).expand_as_secret(&self.options.cipher_suite)?,
        );
        Ok(())
    }

    pub fn remove_encryption_key(&mut self, receiver_id: u64) -> bool {
        self.secrets.remove(&KeyId::from(receiver_id)).is_some()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn remove_key() {
        let mut receiver = Receiver::default();
        assert_eq!(receiver.remove_encryption_key(1234), false);

        receiver.set_encryption_key(4223, b"hendrikswaytoshortpassword").unwrap();
        receiver.set_encryption_key(4711, b"tobismuchbetterpassword;)").unwrap();

        assert!(receiver.remove_encryption_key(4223));
        assert_eq!(receiver.remove_encryption_key(4223), false);

        assert!(receiver.remove_encryption_key(4711));
        assert_eq!(receiver.remove_encryption_key(4711), false);


    }

    #[test]
    fn fail_on_missing_secret() {
        let receiver = Receiver::default();
        // do not set the encryption-key
        let decrypted = receiver.decrypt(b"foobar is unsafe", 0);

        assert_eq!(
            decrypted,
            Err(SframeError::MissingDecryptionKey(KeyId::from(6u8)))
        );
    }
}
