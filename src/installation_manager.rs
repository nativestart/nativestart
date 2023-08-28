extern crate dirs;

use std::fs;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::path::{Path, PathBuf};

use log::*;

use crate::errors::*;
use crate::descriptor::ApplicationArtifact;
use crate::descriptor::ApplicationDescriptor;
use walkdir::WalkDir;
use cluFlock::{FlockLock, SharedFlock};
use crate::validation::validate;

const DESCRIPTOR_FILE_NAME: &str = "app.json";
const LOG_FILE_NAME: &str = "launcher.log";
const BACKUP_DIR: &str = ".launcher.backup";

pub struct InstallationManager {
    root_dir: PathBuf,
}

impl InstallationManager {
    pub fn new(app_id: &'static str) -> Result<InstallationManager> {
        let mut cache_path = dirs::cache_dir()
            .chain_err(|| ErrorKind::StorageError(format!("Could not determine cache directory")))?;
        cache_path.push(app_id);
        fs::create_dir_all(&cache_path)
            .chain_err(|| ErrorKind::StorageError(format!("Could not create installation directory {:?}", &cache_path)))?;

        return Ok(InstallationManager {
            root_dir: cache_path,
        });
    }

    pub fn get_log_file(&self) -> Result<File> {
        let path = self.get_installation_root().join(LOG_FILE_NAME);
        return File::create(&path)
            .chain_err(|| ErrorKind::StorageError(format!("Could not create log file {:?}", &path)));
    }

    pub fn store_descriptor(&self, descriptor: &String) -> Result<()> {
        let path = self.path_for_write(DESCRIPTOR_FILE_NAME)?;
        let mut file = File::create(&path)
            .chain_err(|| ErrorKind::StorageError(format!("Could not create descriptor file {:?}", &path)))?;
        file.write_all(&descriptor.as_bytes())
            .chain_err(|| ErrorKind::StorageError(format!("Could not write descriptor file {:?}", &path)))?;
        return Ok(());
    }

    pub fn get_descriptor(&self) -> Option<String> {
        self.restore_trash(DESCRIPTOR_FILE_NAME).unwrap();
        let path = self.path(DESCRIPTOR_FILE_NAME);

        return match File::open(&path) {
            Ok(mut file) => {
                let mut contents = String::new();
                match file.read_to_string(&mut contents) {
                    Ok(_) => Option::Some(contents),
                    Err(_) => Option::None
                }
            }
            Err(_) => Option::None
        };
    }

    pub fn delete_unused_files(&self, descriptor: &ApplicationDescriptor) -> Result<()> {
        let mut artifact_paths: Vec<PathBuf> = descriptor.artifacts
            .iter()
            .map(|artifact| self.path(artifact))
            .collect();

        // add synthetic artifact path for descriptor and log file to ensure that the file will not be deleted
        artifact_paths.push(self.path(DESCRIPTOR_FILE_NAME));
        artifact_paths.push(self.path(LOG_FILE_NAME));
        
        // manually add artifact path for the splash artifact due it is not included in the main artifacts list
        artifact_paths.push(self.path(&descriptor.splash));

        // add unmanaged paths (like plugins or other user managed directories)
        for path in descriptor.unmanaged_paths.as_ref().unwrap_or(&vec![]) {
            artifact_paths.push(self.path(path));
        }

        let entries_to_delete: Vec<PathBuf> = self.get_paths_to_delete(self.get_installation_root().as_path(), &artifact_paths)?;

        for entry_path in entries_to_delete {
            if entry_path.exists() {
                if entry_path.is_file() {
                    fs::remove_file(&entry_path)
                        .chain_err(|| ErrorKind::StorageError(format!("Could not remove unused file {:?}", &entry_path)))?;
                } else {
                    fs::remove_dir_all(&entry_path)
                        .chain_err(|| ErrorKind::StorageError(format!("Could not remove unused directory {:?}", &entry_path)))?;
                }
            }
        }
        return Ok(());
    }

    fn get_paths_to_delete(&self, root: &Path, artifact_paths: &Vec<PathBuf>) -> Result<Vec<PathBuf>> {
        let mut entries_to_delete: Vec<PathBuf> = Vec::new();

        let dir = fs::read_dir(root)
            .chain_err(|| ErrorKind::StorageError(format!("Could not read directory {:?}", &root)))?;

        for entry in dir {
            let entry_path = entry?.path();

            let mut exact_match = false;
            let mut partial_match = false;

            for artifact_path in artifact_paths.iter() {
                if artifact_path.eq(&entry_path.to_path_buf()) {
                    exact_match = true;
                    break;
                }
                if artifact_path.starts_with(&entry_path) {
                    partial_match = true;
                    break;
                }
            }

            if !exact_match && !partial_match {
                entries_to_delete.push(entry_path.to_path_buf());
            } else if !exact_match {
                entries_to_delete.append(&mut self.get_paths_to_delete(entry_path.as_path(), artifact_paths)?);
            }
        }

        return Ok(entries_to_delete);
    }

    pub fn get_files_to_download(&self, artifacts: &Vec<ApplicationArtifact>) -> Vec<ApplicationArtifact> {
        let mut result: Vec<ApplicationArtifact> = Vec::new();
        for artifact in artifacts {
            info!("Checking {}", artifact.path);

            self.restore_trash(artifact).unwrap();
            let path = self.path(artifact);

            if !validate(artifact, path.as_path()) {
                result.push(artifact.clone());
            }
        }
        return result;
    }

    pub fn lock_installation(&self, descriptor: &ApplicationDescriptor) -> Result<Vec<FlockLock<File>>> {
        let mut paths: Vec<PathBuf> = Vec::new();

        for artifact in descriptor.all_artifacts() {
            let path = self.path(&artifact).clone();
            if artifact.is_archive() {
                for entry in WalkDir::new(path.as_path())
                    .into_iter()
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| match entry.metadata() {
                        Ok(metadata) => metadata.is_file(),
                        Err(_) => false
                    }) {

                    paths.push(entry.into_path());
                }
            } else {
                paths.push(path);
            }
        }

        paths.push(self.path(DESCRIPTOR_FILE_NAME));

        let files = paths.into_iter().map(|path|
            SharedFlock::wait_lock(File::open(path).unwrap()).unwrap()
        ).collect();

        return Ok(files);
    }

    pub fn verify_installation(&self, descriptor: &ApplicationDescriptor) -> bool {
        return self.get_files_to_download(&descriptor.artifacts).is_empty();
    }

    pub fn unlock_files(&self, files: Vec<FlockLock<File>>) -> Result<()> {
        for file in files {
            file.unlock_no_result();
        }
        return Ok(());
    }

    pub fn get_installation_root(&self) -> PathBuf {
        return self.root_dir.clone();
    }

    pub fn path_for_write<P: AsRef<Path>>(&self, artifact: P) -> Result<PathBuf> {
        self.move_to_trash(&artifact)?;
        return Ok(self.path(&artifact));
    }

    fn path<P: AsRef<Path>>(&self, artifact: P) -> PathBuf {
        let mut path = self.root_dir.clone();
        path.push(&artifact);
        return path;
    }

    fn backup_path<P: AsRef<Path>>(&self, artifact: P) -> PathBuf {
        let mut path = self.root_dir.clone();
        path.push(BACKUP_DIR);
        path.push(&artifact);
        return path;
    }

    fn move_to_trash<P: AsRef<Path>>(&self, artifact: P) -> Result<()> {
        let path = self.path(&artifact);
        if path.exists() {
            let backup_path = self.backup_path(&artifact);
            if backup_path.exists() {
                if backup_path.is_file() {
                    fs::remove_file(&backup_path)?;
                } else {
                    fs::remove_dir_all(&backup_path)?;
                }
            }
            fs::create_dir_all(backup_path.parent().unwrap())
                .chain_err(|| ErrorKind::StorageError(format!("Could not create backup directory for {:?}", &backup_path)))?;
            fs::rename(&path, &self.backup_path(&artifact))
                .chain_err(|| ErrorKind::StorageError(format!("Could not backup {:?}", &path)))?;
        }
        return Ok(());
    }

    fn restore_trash<P: AsRef<Path>>(&self, artifact: P) -> Result<()>{
        let backup_path = self.backup_path(&artifact);
        let path = self.path(&artifact);
        if backup_path.exists() {
            if path.exists() {
                if path.is_file() {
                    fs::remove_file(&path)?;
                } else {
                    fs::remove_dir_all(&path)?;
                }
            }
            fs::rename(&backup_path, &path)?;
        }
        return Ok(());
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crate::installation_manager::{InstallationManager, DESCRIPTOR_FILE_NAME};
    use std::fs::File;
    use std::io::{Write, Read};
    use tempfile::TempDir;
    use crate::descriptor::ApplicationArtifact;

    #[test]
    fn test_empty() {
        let (temp_dir, installation) = setup();
        let path = temp_dir.into_path();

        let entries_to_delete = installation.get_paths_to_delete(path.as_path(), &vec![]).unwrap();

        assert_eq!(true, entries_to_delete.is_empty());
    }

    #[test]
    fn test_one_missing_file() {
        let (temp_dir, installation) = setup();
        let path = temp_dir.into_path();

        let mut missing_file = path.clone();
        missing_file.push("missing.file");
        let artifacts = vec![missing_file];

        let entries_to_delete = installation.get_paths_to_delete(path.as_path(), &artifacts).unwrap();

        assert_eq!(true, entries_to_delete.is_empty());
    }

    #[test]
    fn test_one_missing_dir() {
        let (temp_dir, installation) = setup();
        let path = temp_dir.into_path();

        let mut missing_dir = path.clone();
        missing_dir.push("missing_dir/");
        let artifacts = vec![missing_dir];

        let entries_to_delete = installation.get_paths_to_delete(path.as_path(), &artifacts).unwrap();

        assert_eq!(true, entries_to_delete.is_empty());
    }

    #[test]
    fn test_one_needless_dir() {
        let (temp_dir, installation) = setup();
        let path = temp_dir.into_path();

        let mut needless_path: PathBuf = path.clone();
        needless_path.push("needless_dir");
        fs::create_dir(&needless_path).unwrap();

        let entries_to_delete = installation.get_paths_to_delete(path.as_path(), &vec![]).unwrap();

        assert_eq!(false, entries_to_delete.is_empty());
        assert_entries_to_delete(&path, &vec![String::from("needless_dir")], &entries_to_delete);
    }

    #[test]
    fn test_one_needless_file() {
        let (temp_dir, installation) = setup();
        let path = temp_dir.into_path();

        let mut needless_path: PathBuf = path.clone();
        needless_path.push("needless.file");
        fs::File::create(&needless_path).unwrap();

        let entries_to_delete = installation.get_paths_to_delete(path.as_path(), &vec![]).unwrap();

        assert_eq!(false, entries_to_delete.is_empty());
        assert_entries_to_delete(&path, &vec![String::from("needless.file")], &entries_to_delete);
    }

    #[test]
    fn test_one_needless_file_in_subdir() {
        let (temp_dir, installation) = setup();
        let path = temp_dir.into_path();

        let mut needless_path: PathBuf = path.clone();
        needless_path.push("dir/needless.file");
        fs::create_dir_all(needless_path.parent().unwrap()).unwrap();
        fs::File::create(&needless_path).unwrap();

        let mut missing_file = path.clone();
        missing_file.push("dir/missing.file");
        let artifacts = vec![missing_file];

        let entries_to_delete = installation.get_paths_to_delete(path.as_path(), &artifacts).unwrap();

        assert_eq!(false, entries_to_delete.is_empty());
        assert_entries_to_delete(&path, &vec![String::from("dir/needless.file")], &entries_to_delete);
    }

    #[test]
    fn test_one_needless_dir_in_subdir() {
        let (temp_dir, installation) = setup();
        let path = temp_dir.into_path();

        let mut needless_path: PathBuf = path.clone();
        needless_path.push("dir/needless_dir");
        fs::create_dir_all(&needless_path).unwrap();

        let mut missing_dir = path.clone();
        missing_dir.push("dir/missing_dir");
        let artifacts = vec![missing_dir];

        let entries_to_delete = installation.get_paths_to_delete(path.as_path(), &artifacts).unwrap();

        assert_eq!(false, entries_to_delete.is_empty());
        assert_entries_to_delete(&path, &vec![String::from("dir/needless_dir")], &entries_to_delete);
    }

    fn assert_entries_to_delete(root: &PathBuf, expected_entries_to_delete: &Vec<String>, entries_to_delete: &Vec<PathBuf>) {
        let expected_entries_to_delete: Vec<PathBuf> = expected_entries_to_delete.iter().map(|entry| {
            let mut path = root.clone();
            path.push(entry.clone());
            return path;
        }).collect();
        assert_eq!(&expected_entries_to_delete, entries_to_delete);
    }

    #[test]
    fn test_restore_descriptor() {
        let (_, installation) = setup();

        let backup = installation.backup_path(DESCRIPTOR_FILE_NAME);
        fs::create_dir_all(backup.parent().unwrap()).unwrap();
        File::create(&backup).unwrap().write_all("OK".as_bytes()).unwrap();

        let orig = installation.path(DESCRIPTOR_FILE_NAME);
        File::create(&orig).unwrap().write_all("not OK".as_bytes()).unwrap();

        assert_eq!("OK", installation.get_descriptor().unwrap());
    }

    #[test]
    fn test_backup_restore() {
        let (_, installation) = setup();

        let backup = installation.backup_path("lib/artifact.jar");
        fs::create_dir_all(backup.parent().unwrap()).unwrap();
        File::create(&backup).unwrap().write_all("old".as_bytes()).unwrap();

        let orig = installation.path("lib/artifact.jar");
        fs::create_dir_all(orig.parent().unwrap()).unwrap();
        File::create(&orig).unwrap().write_all("OK".as_bytes()).unwrap();

        installation.move_to_trash("lib/artifact.jar").unwrap();

        let artifacts: Vec<ApplicationArtifact> = vec!(ApplicationArtifact {
            path: String::from("lib/artifact.jar"),
            url: String::from("http://host/file"),
            checksum: String::from(""),
            download_size: Some(50),
            size: 123,
        });
        // trigger restore
        installation.get_files_to_download(&artifacts);

        let mut contents = String::new();
        File::open(&orig).unwrap().read_to_string(&mut contents).unwrap();
        assert_eq!("OK", contents);
    }

    fn setup() -> (TempDir, InstallationManager) {
        let temporary_dir = tempfile::tempdir().unwrap();
        let path = temporary_dir.path();

        let installation_manager = InstallationManager {
            root_dir: PathBuf::from(path)
        };
        return (temporary_dir, installation_manager);
    }
}