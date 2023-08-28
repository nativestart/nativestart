mod existence;
mod size;
mod checksum;

use std::path::Path;
use crate::descriptor::ApplicationArtifact;

trait ApplicationArtifactValidator {
    fn is_valid(&self, application_artifact: &ApplicationArtifact, file_path: &Path) -> bool;
}

pub fn validate(application_artifact: &ApplicationArtifact, file_path: &Path) -> bool {
    let existence_validator = existence::ExistenceApplicationArtifactValidator {};
    if !existence_validator.is_valid(application_artifact, file_path) {
        return false;
    }
    let size_validator = size::SizeApplicationArtifactValidator{};
    if !size_validator.is_valid(application_artifact, file_path) {
        return false;
    }
    let checksum_validator = checksum::ChecksumApplicationArtifactValidator{};
    if !checksum_validator.is_valid(application_artifact, file_path) {
        return false;
    }
    return true;
}