use std::path::Path;
use serde_derive::*;
use log::*;
use crate::errors::*;

#[cfg(feature = "check-signature")]
use ring::signature;

#[derive(Deserialize, Debug)]
pub struct ApplicationDescriptor {
    pub name: String,
    pub version: String,
    pub signature: Option<String>,
    pub splash: ApplicationComponent,
    #[serde(rename="jvm")]
    pub jvm_params: JvmParameters,
    #[serde(rename="component")]
    pub components: Vec<ApplicationComponent>,
    #[serde(rename="unmanaged")]
    pub unmanaged_paths: Option<Vec<String>>
}

impl ApplicationDescriptor {
    pub fn parse(content: &str, public_key: Option<[u8; 32]>) -> Result<ApplicationDescriptor> {
        let descriptor: Result<ApplicationDescriptor> = toml::from_str(&content).map_err(|e| {
            error!("Descriptor is invalid:\n{}", content);
            ErrorKind::InvalidDescriptor(e.to_string()).into()
        });

        // check signature if required
        match descriptor {
            Ok(desc) => {
                for component in &desc.all_components() {
                    if component.path.contains("..") {
                        panic!("Descriptor defines storage location outside application directory. Please inform author about this security incident!");
                    }
                }
                if public_key.is_some() {
                    return ApplicationDescriptor::verify(content, &desc.signature, public_key.unwrap())
                        .map(|_| desc);
                } else if desc.signature.is_some() {
                    return Err(ErrorKind::SignatureError("Signature is present but not supported by launcher".to_string()).into());
                } else {
                    return Ok(desc);
                }
            }
            Err(e) => {
                return Err(e);
            }
        }
    }

    pub fn all_components(&self) -> Vec<&ApplicationComponent> {
        let mut component = Vec::new();
        component.extend(&self.components);
        component.push(&self.splash);
        return component;
    }

    #[cfg(not(feature = "check-signature"))]
    fn verify(_content: &str, _signature: &Option<String>, _public_key: [u8; 32]) -> Result<()> {
        // no signature checking available
        error!("Signature feature has not been enabled during compilation, but public key has been defined");
        return Err(ErrorKind::SignatureError("Signature feature has not been enabled during compilation".to_string()).into());
    }

    #[cfg(feature = "check-signature")]
    fn verify(content: &str, signature: &Option<String>, public_key: [u8; 32]) -> Result<()> {
        match signature {
            None => {
                error!("Signature is missing in application descriptor");
                return Err(ErrorKind::SignatureError("Signature is missing".to_string()).into());
            }
            Some(signature) => {
                // remove signature from content to get normalized content
                let mut normalized_content = String::from(content);
                normalized_content = normalized_content.replace(signature.as_str(), "");

                let sig_bytes = hex::decode(signature).unwrap();
                let key =
                    signature::UnparsedPublicKey::new(&signature::ED25519, public_key);
                let signature_check = key.verify(&normalized_content.as_bytes(), &sig_bytes);
                if signature_check.is_ok() {
                    return Ok(());
                } else {
                    error!("Signature is invalid");
                    return Err(ErrorKind::SignatureError(signature_check.err().unwrap().to_string()).into())
                }
            }
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct JvmParameters {
    #[serde(rename="path")]
    pub jvm_path: String,
    #[serde(rename="library")]
    pub jvm_library: String,
    #[serde(rename="main")]
    pub main_class: String,
    pub options: Vec<String>,
}

#[derive(Deserialize, Debug)]
#[derive(Clone)]
pub struct ApplicationComponent {
    pub url: String,
    pub size: u64,
    pub download_size: Option<u64>,
    pub checksum: String,
    pub path: String,
    pub cache_path: Option<String>,
}

impl ApplicationComponent {
    pub fn is_archive(&self) -> bool {
        self.path.ends_with("/")
    }
}

impl AsRef<Path> for ApplicationComponent {
    fn as_ref(&self) -> &Path {
        return Path::new(&self.path);
    }
}


#[cfg(test)]
#[cfg(feature = "check-signature")]
mod tests {
    use hex::ToHex;
    use ring::{rand, signature};
    use ring::signature::KeyPair;
    use super::ApplicationDescriptor;

    #[test]
    fn test_signature_verification() {
        let rng = rand::SystemRandom::new();
        let pkcs8_bytes = signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = signature::Ed25519KeyPair::from_pkcs8(pkcs8_bytes.as_ref()).unwrap();

        let content = "Hello World";
        let signature: String = key_pair.sign(content.as_bytes()).encode_hex();

        let tmp = key_pair.public_key().as_ref();

        let mut peer_public_key_bytes= [0; 32];
        for i in 0..32 {
            peer_public_key_bytes[i] = tmp[i];
        }

        let result = ApplicationDescriptor::verify(&content, &Some(String::from(signature)), peer_public_key_bytes);
        assert_eq!(true, result.is_ok());
    }
}
