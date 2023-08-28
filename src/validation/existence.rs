use std::path::Path;

use crate::descriptor::ApplicationArtifact;
use super::ApplicationArtifactValidator;

pub struct ExistenceApplicationArtifactValidator {}

impl ApplicationArtifactValidator for ExistenceApplicationArtifactValidator {
    fn is_valid(&self, _application_artifact: &ApplicationArtifact, file_path: &Path) -> bool {
        return file_path.exists();
    }
}

#[cfg(test)]
mod tests {
    use crate::descriptor::ApplicationArtifact;
    use std::path::PathBuf;
    use super::ApplicationArtifactValidator;
    use std::fs::File;

    #[test]
    fn test_invalid_if_not_exists() {
        let application_artifact = create_application_artifact();

        let mut path = PathBuf::new();
        path.push("non");
        path.push("existing");
        path.push("path");

        let validator: Box<dyn ApplicationArtifactValidator> = Box::new(super::ExistenceApplicationArtifactValidator {});
        assert_eq!(false, validator.is_valid(&application_artifact, path.as_path()));
    }

    #[test]
    fn test_valid_if_exists() {
        let application_artifact = create_application_artifact();

        let temporary_dir = tempfile::tempdir().unwrap();
        let mut path = temporary_dir.into_path();
        path.push("test.jar");

        File::create(&path).unwrap();

        let validator: Box<dyn ApplicationArtifactValidator> = Box::new(super::ExistenceApplicationArtifactValidator {});
        assert_eq!(true, validator.is_valid(&application_artifact, path.as_path()));
    }

    fn create_application_artifact() -> ApplicationArtifact {
        return ApplicationArtifact {
            path: String::from("relative/path"),
            url: String::from("http://host/file"),
            checksum: String::from("any checksum"),
            download_size: Some(50),
            size: 123,
        };
    }
}