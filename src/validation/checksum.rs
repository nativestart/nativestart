use std::fs;
use std::io;
use std::path::Path;
use std::collections::BTreeMap;

use walkdir::WalkDir;
use log::*;

#[cfg(not(feature = "checksum-blake3"))]
use sha2::{Sha256, Digest};

#[cfg(feature = "checksum-blake3")]
use blake3::Hasher;


use crate::errors::*;
use crate::descriptor::ApplicationArtifact;
use super::ApplicationArtifactValidator;

#[cfg(not(feature = "checksum-blake3"))]
type ChecksumHasher = Sha256;


#[cfg(feature = "checksum-blake3")]
type ChecksumHasher = Hasher;

pub struct ChecksumApplicationArtifactValidator {}

impl ApplicationArtifactValidator for ChecksumApplicationArtifactValidator {
    fn is_valid(&self, application_artifact: &ApplicationArtifact, file_path: &Path) -> bool {
        let hash = if application_artifact.is_archive() {
            let mut hashes = BTreeMap::new();

            for entry in WalkDir::new(file_path)
                .into_iter()
                .filter_map(|entry| entry.ok())
                .filter(|entry| match entry.metadata() {
                    Ok(metadata) => !metadata.is_dir(),
                    Err(_) => false
                }) {

                let hash = match hash(entry.path()) {
                    Ok(h) => h,
                    Err(_) => return false
                };
                let path = String::from(entry.path().strip_prefix(file_path).unwrap()
                    .to_str().unwrap()
                    .replace("\\", "/"));
                hashes.insert(path, hash);
            }
            let mut hasher = create_hasher();
            for (path, hash) in &hashes {
                hasher.update(path.as_bytes());
                hasher.update(b"\t");
                hasher.update(hash.as_bytes());
                hasher.update(b"\n");
            }
            finalize(hasher)
        } else {
            match hash(file_path) {
                Ok(h) => h,
                Err(_) => return false
            }
        };

        let hash_match = hash.as_str().eq(&application_artifact.checksum);
        if !hash_match {
            debug!("The hash of {} is {}, but should be {}", application_artifact.path, hash, application_artifact.checksum);
        }
        return hash_match;
    }
}

#[cfg(not(feature = "checksum-blake3"))]
fn create_hasher() -> ChecksumHasher {
    return Sha256::new();
}

#[cfg(feature = "checksum-blake3")]
fn create_hasher() -> ChecksumHasher {
    return blake3::Hasher::new();
}

fn hash(file_path: &Path) -> Result<String> {
    debug!("Hashing {:?}", file_path);
    let mut hasher = create_hasher();
    match fs::read_link(file_path) {
        Ok(target) => hasher.update(target.as_path().to_str().unwrap().as_bytes()),
        Err(_e) => {
            let mut file = fs::File::open(file_path)?;
            io::copy(&mut file, &mut hasher)?;
        }
    }
    Ok(finalize(hasher))
}

#[cfg(not(feature = "checksum-blake3"))]
fn finalize(hasher: ChecksumHasher) -> String {
    return format!("{:x}", hasher.finalize());
}

#[cfg(feature = "checksum-blake3")]
fn finalize(hasher: ChecksumHasher) -> String {
    return String::from(hasher.finalize().to_hex().as_str());
}


#[cfg(test)]
mod tests {
    use crate::descriptor::ApplicationArtifact;
    use super::ApplicationArtifactValidator;
    use std::fs::File;

    static EXPECTED_HASH: &str = "d29751f2649b32ff572b5e0a9f541ea660a50f94ff0beedfb0b692b924cc8025";

    #[test]
    fn test_invalid_if_wrong_checksum() {
        let application_artifact = create_application_artifact(String::from(EXPECTED_HASH));

        let temporary_dir = tempfile::tempdir().unwrap();
        let mut path = temporary_dir.into_path();
        path.push("test.jar");

        let temporary_file = File::create(&path).unwrap();
        temporary_file.set_len(1000001).unwrap();

        let validator: Box<dyn ApplicationArtifactValidator> = Box::new(super::ChecksumApplicationArtifactValidator {});
        assert_eq!(false, validator.is_valid(&application_artifact, path.as_path()));
    }

    #[test]
    fn test_valid_if_correct_checksum() {
        let application_artifact = create_application_artifact(String::from(EXPECTED_HASH));

        let temporary_dir = tempfile::tempdir().unwrap();
        let mut path = temporary_dir.into_path();
        path.push("test.jar");

        let temporary_file = File::create(&path).unwrap();
        temporary_file.set_len(1000000).unwrap();

        let validator: Box<dyn ApplicationArtifactValidator> = Box::new(super::ChecksumApplicationArtifactValidator {});
        assert_eq!(true, validator.is_valid(&application_artifact, path.as_path()));
    }

    fn create_application_artifact(checksum: String) -> ApplicationArtifact {
        return ApplicationArtifact {
            path: String::from("relative/path"),
            url: String::from("http://host/file"),
            checksum,
            download_size: Some(50),
            size: 123,
        };
    }
}