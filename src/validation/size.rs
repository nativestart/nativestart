use std::path::Path;
use std::fs;

use log::*;
use walkdir::WalkDir;

use crate::descriptor::ApplicationArtifact;
use super::ApplicationArtifactValidator;

pub struct SizeApplicationArtifactValidator {}

impl ApplicationArtifactValidator for SizeApplicationArtifactValidator {
    fn is_valid(&self, application_artifact: &ApplicationArtifact, file_path: &Path) -> bool {
        if application_artifact.is_archive() {
            let total_size = WalkDir::new(file_path)
                .follow_links(false)
                .into_iter()
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| entry.metadata().ok())
                .filter(|metadata| metadata.is_file())
                .fold(0, |acc, m| acc + m.len());

            let size_matches = total_size.eq(&application_artifact.size);
            if !size_matches {
                debug!("{} has size of {} (expected: {})", application_artifact.path, total_size, application_artifact.size);
            }
            return size_matches;
        } else {
            let len = fs::metadata(file_path).and_then(|m| Ok(m.len())).unwrap_or(0);
            let size_matches = len.eq(&application_artifact.size);
            if !size_matches {
                debug!("{} has size of {} (expected: {})", application_artifact.path, len, application_artifact.size);
            }
            return size_matches;
        }
    }
}


#[cfg(test)]
mod tests {
    use crate::descriptor::ApplicationArtifact;
    use super::ApplicationArtifactValidator;
    use std::fs::File;

    #[test]
    fn test_invalid_if_too_large() {
        let application_artifact = create_application_artifact(1000000);

        let temporary_dir = tempfile::tempdir().unwrap();
        let mut path = temporary_dir.into_path();
        path.push("test.jar");

        let temporary_file = File::create(&path).unwrap();
        temporary_file.set_len(1000001).unwrap();

        let validator: Box<dyn ApplicationArtifactValidator> = Box::new(super::SizeApplicationArtifactValidator {});
        assert_eq!(false, validator.is_valid(&application_artifact, path.as_path()));
    }

    #[test]
    fn test_invalid_if_too_small() {
        let application_artifact = create_application_artifact(1000000);

        let temporary_dir = tempfile::tempdir().unwrap();
        let mut path = temporary_dir.into_path();
        path.push("test.jar");

        let temporary_file = File::create(&path).unwrap();
        temporary_file.set_len(999999).unwrap();

        let validator: Box<dyn ApplicationArtifactValidator> = Box::new(super::SizeApplicationArtifactValidator {});
        assert_eq!(false, validator.is_valid(&application_artifact, path.as_path()));
    }

    #[test]
    fn test_valid_if_same_size() {
        let application_artifact = create_application_artifact(1000000);

        let temporary_dir = tempfile::tempdir().unwrap();
        let mut path = temporary_dir.into_path();
        path.push("test.jar");

        let temporary_file = File::create(&path).unwrap();
        temporary_file.set_len(1000000).unwrap();

        let validator: Box<dyn ApplicationArtifactValidator> = Box::new(super::SizeApplicationArtifactValidator {});
        assert_eq!(true, validator.is_valid(&application_artifact, path.as_path()));
    }

    fn create_application_artifact(size: u64) -> ApplicationArtifact {
        return ApplicationArtifact {
            path: String::from("relative/path"),
            url: String::from("http://host/file"),
            checksum: String::from("any checksum"),
            download_size: Some(50),
            size,
        };
    }
}