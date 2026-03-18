use std::collections::BTreeMap;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
extern crate dirs;

use std::fs;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::path::{Path, PathBuf};
use blake3::Hasher;
use log::*;

use crate::errors::*;
use crate::descriptor::ApplicationComponent;
use crate::descriptor::ApplicationDescriptor;
use walkdir::WalkDir;
use cluFlock::{FlockLock, SharedFlock, ExclusiveFlock};
use rayon::prelude::IntoParallelIterator;
use crate::installation_manager::CheckResult::{NotOk, OkLocked};

const DESCRIPTOR_FILE_NAME: &str = "app.toml";
const LOG_FILE_NAME: &str = "launcher.log";
const BACKUP_DIR: &str = ".launcher.backup";

pub struct InstallationManager {
    root_dir: PathBuf,
}

pub enum CheckResult {
    OkLocked(Vec<FlockLock<File>>),
    NotOk(ApplicationComponent)
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

    pub fn is_descriptor_locked(&self) -> Result<bool> {
        let path = self.path(DESCRIPTOR_FILE_NAME);
        if !path.exists() {
           return Ok(false);
        }
        match ExclusiveFlock::try_lock(File::open(path)?) {
            Ok(_) => Ok(false),
            Err(_) => Ok(true)
        }
    }

    pub fn store_descriptor(&self, descriptor: &String) -> Result<()> {
        let path = self.path_for_write(DESCRIPTOR_FILE_NAME)?;
        let mut file = File::create(&path)
            .chain_err(|| ErrorKind::StorageError(format!("Could not create descriptor file {:?}", &path)))?;
        file.write_all(&descriptor.as_bytes())
            .chain_err(|| ErrorKind::StorageError(format!("Could not write descriptor file {:?}", &path)))?;
        return Ok(());
    }

    pub fn lock_descriptor(&self) -> Result<FlockLock<File>> {
        let path = self.path(DESCRIPTOR_FILE_NAME);
        return Ok(SharedFlock::wait_lock(File::open(path)?).unwrap());
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

    pub fn create_unmanaged(&self, descriptor: &ApplicationDescriptor) -> Result<()> {
        for path in descriptor.unmanaged_paths.as_ref().unwrap_or(&vec![]) {
            let path = self.get_installation_root().join(path);
            fs::create_dir_all(&path)
                .chain_err(|| ErrorKind::StorageError(format!("Could not create directory {:?}", &path)))?;
        }
        Ok(())
    }

    pub fn delete_unused_files(&self, descriptor: &ApplicationDescriptor) -> Result<()> {
        let mut component_paths: Vec<PathBuf> = descriptor.components
            .iter()
            .map(|component| self.path(component))
            .collect();

        // add synthetic component path for descriptor and log file to ensure that the file will not be deleted
        component_paths.push(self.path(DESCRIPTOR_FILE_NAME));
        component_paths.push(self.path(LOG_FILE_NAME));
        
        // manually add component path for the splash component due it is not included in the main components list
        component_paths.push(self.path(&descriptor.splash));

        // add unmanaged paths (like plugins or other user managed directories)
        for path in descriptor.unmanaged_paths.as_ref().unwrap_or(&vec![]) {
            component_paths.push(self.path(path));
        }
        // add cache paths and create them if they do not yet exist
        for component in &descriptor.components {
            if component.cache_path.is_some() {
                let path = self.path(component.cache_path.as_ref().unwrap());
                if !path.exists() {
                    fs::create_dir_all(&path)?;
                }
                component_paths.push(path);
            }
        }

        let entries_to_delete: Vec<PathBuf> = self.get_paths_to_delete(self.get_installation_root().as_path(), &component_paths)?;

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

    fn get_paths_to_delete(&self, root: &Path, component_paths: &Vec<PathBuf>) -> Result<Vec<PathBuf>> {
        let mut entries_to_delete: Vec<PathBuf> = Vec::new();

        let dir = fs::read_dir(root)
            .chain_err(|| ErrorKind::StorageError(format!("Could not read directory {:?}", &root)))?;

        for entry in dir {
            let entry_path = entry?.path();

            let mut exact_match = false;
            let mut partial_match = false;

            for component_path in component_paths.iter() {
                if component_path.eq(&entry_path.to_path_buf()) {
                    exact_match = true;
                    break;
                }
                if component_path.starts_with(&entry_path) {
                    partial_match = true;
                    break;
                }
            }

            if !exact_match && !partial_match {
                entries_to_delete.push(entry_path.to_path_buf());
            } else if !exact_match {
                entries_to_delete.append(&mut self.get_paths_to_delete(entry_path.as_path(), component_paths)?);
            }
        }

        return Ok(entries_to_delete);
    }

    pub fn restore_backup(&self, components: &Vec<ApplicationComponent>) {
        for component in components {
            self.restore_trash(&component).unwrap();
        }
    }

    pub fn check_component(&self, component: ApplicationComponent) -> CheckResult {
        info!("Checking {}", component.path);
        let path = self.path(&component);

        if !path.exists() {
            NotOk(component)
        } else if self.size(&path) != component.size {
            info!("The size of {} is {}, but should be {}", &component.path, self.size(&path), &component.size);
            NotOk(component)
        } else {
            let files = self.lock(&path);
            let hash = if path.is_dir() {self.hash_dir(&path, &files)} else {self.hash_file(&path)};
            let hash_match = hash.as_str().eq(&component.checksum);
            if !hash_match {
                info!("The hash of {} is {}, but should be {}", &component.path, hash, &component.checksum);
                self.unlock(files);
                NotOk(component)
            } else {
                let mut locks: Vec<FlockLock<File>> = Vec::new();
                for file in files {
                    locks.push(file.1);
                }
                OkLocked(locks)
            }
        }
    }

    pub fn check_components(&self, components: &Vec<ApplicationComponent>) -> Vec<CheckResult> {
        components.into_par_iter().cloned().map(|component| {
            self.check_component(component)
        }).collect()
    }

    fn size(&self, file_path: &Path) -> u64 {
        if file_path.is_dir() {
            WalkDir::new(file_path)
                .follow_links(false)
                .into_iter()
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| entry.metadata().ok())
                .filter(|metadata| metadata.is_file())
                .fold(0, |acc, m| acc + m.len())
        } else {
            fs::metadata(file_path).and_then(|m| Ok(m.len())).unwrap_or(0)
        }
    }

    fn lock(&self, file_path: &Path) -> Vec<(PathBuf, FlockLock<File>)> {
        if file_path.is_dir() {
            WalkDir::new(file_path)
                .into_iter()
                .filter_map(|entry| entry.ok())
                .filter(|entry| match entry.metadata() {
                    Ok(metadata) => !metadata.is_dir(),
                    Err(_) => false
                })
                .map(|path|
                         (path.clone().into_path(), SharedFlock::wait_lock(File::open(path.into_path()).unwrap()).unwrap()))
                .collect()
        } else {
            vec!((file_path.to_path_buf(), SharedFlock::wait_lock(File::open(file_path).unwrap()).unwrap()))
        }
    }

    pub fn unlock(&self, files : Vec<(PathBuf, FlockLock<File>)>) {
        for file in files {
            file.1.unlock_no_err_result();
        }
    }

    fn hash_dir(&self, file_path: &Path, files : &Vec<(PathBuf, FlockLock<File>)>) -> String {
        let hash_vec : Vec<_> = files.par_iter().filter_map(|(file, _)| {
            let hash = self.hash_file(file);
            let path = String::from(file.strip_prefix(file_path).unwrap()
                .to_str().unwrap()
                .replace("\\", "/"));
            Some((path, hash))
        }).collect();

        let mut hashes = BTreeMap::new();
        for (path, hash) in hash_vec {
            hashes.insert(path, hash);
        }
        let mut hasher = Hasher::new();
        for (path, hash) in &hashes {
            hasher.update(path.as_bytes());
            hasher.update(b"\t");
            hasher.update(hash.as_bytes());
            hasher.update(b"\n");
        }
        String::from(hasher.finalize().to_hex().as_str())
    }

    fn hash_file(&self, file_path: &Path) -> String {
        debug!("Hashing {:?}", file_path);
        let mut hasher = Hasher::new();
        match fs::read_link(file_path) {
            Ok(target) => hasher.update(target.as_path().to_str().unwrap().as_bytes()),
            Err(_e) => {
                hasher.update_reader(File::open(file_path).unwrap()).unwrap()
            }
        };
        String::from(hasher.finalize().to_hex().as_str())
    }

    pub fn unlock_files(&self, files: Vec<FlockLock<File>>) -> Result<()> {
        for file in files {
            file.unlock_no_err_result();
        }
        return Ok(());
    }

    pub fn get_installation_root(&self) -> PathBuf {
        return self.root_dir.clone();
    }

    pub fn path_for_write<P: AsRef<Path>>(&self, component: P) -> Result<PathBuf> {
        self.move_to_trash(&component)?;
        return Ok(self.path(&component));
    }

    pub fn recreate_dir<P: AsRef<Path>>(&self, component: P) -> Result<()> {
        let path = self.path(&component);
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        return Ok(());
    }

    fn path<P: AsRef<Path>>(&self, component: P) -> PathBuf {
        let mut path = self.root_dir.clone();
        path.push(&component);
        return path;
    }

    fn backup_path<P: AsRef<Path>>(&self, component: P) -> PathBuf {
        let mut path = self.root_dir.clone();
        path.push(BACKUP_DIR);
        path.push(&component);
        return path;
    }

    fn move_to_trash<P: AsRef<Path>>(&self, component: P) -> Result<()> {
        let path = self.path(&component);
        if path.exists() {
            let backup_path = self.backup_path(&component);
            if backup_path.exists() {
                if backup_path.is_file() {
                    fs::remove_file(&backup_path)?;
                } else {
                    fs::remove_dir_all(&backup_path)?;
                }
            }
            fs::create_dir_all(backup_path.parent().unwrap())
                .chain_err(|| ErrorKind::StorageError(format!("Could not create backup directory for {:?}", &backup_path)))?;
            fs::rename(&path, &self.backup_path(&component))
                .chain_err(|| ErrorKind::StorageError(format!("Could not backup {:?}", &path)))?;
        }
        return Ok(());
    }

    fn restore_trash<P: AsRef<Path>>(&self, component: P) -> Result<()>{
        let backup_path = self.backup_path(&component);
        let path = self.path(&component);
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
    use crate::descriptor::ApplicationComponent;

    #[test]
    fn test_size_hash_single_file() {
        let (temp_dir, installation) = setup();
        let mut path = temp_dir.keep();
        path.push("test.jar");

        let mut temporary_file = File::create(&path).unwrap();
        temporary_file.write_all(b"test").unwrap();

        assert_eq!(4, installation.size(path.as_path()));
        assert_eq!("4878ca0425c739fa427f7eda20fe845f6b2e46ba5fe2a14df5b1e32f50603215", installation.hash_file(path.as_path()));
    }

    #[test]
    fn test_size_hash_directory() {
        let (temp_dir, installation) = setup();
        let path = temp_dir.keep();
        File::create(&path.join("test.jar")).unwrap().write_all(b"test").unwrap();
        File::create(&path.join("main.jar")).unwrap().write_all(b"main").unwrap();
        let subdir = path.join("subdir");
        fs::create_dir(&subdir).unwrap();
        File::create(&subdir.join("test.txt")).unwrap().write_all(b"sub").unwrap();

        assert_eq!(11, installation.size(path.as_path()));
        let files = installation.lock(&path);
        assert_eq!(3, files.len());
        assert_eq!("a1911db12774eca1371894923dd3870595d52185797e43972e808a901555faa1", installation.hash_dir(path.as_path(), &files));
        installation.unlock(files);
    }

    #[test]
    fn test_empty() {
        let (temp_dir, installation) = setup();
        let path = temp_dir.keep();

        let entries_to_delete = installation.get_paths_to_delete(path.as_path(), &vec![]).unwrap();

        assert_eq!(true, entries_to_delete.is_empty());
    }

    #[test]
    fn test_one_missing_file() {
        let (temp_dir, installation) = setup();
        let path = temp_dir.keep();

        let mut missing_file = path.clone();
        missing_file.push("missing.file");
        let components = vec![missing_file];

        let entries_to_delete = installation.get_paths_to_delete(path.as_path(), &components).unwrap();

        assert_eq!(true, entries_to_delete.is_empty());
    }

    #[test]
    fn test_one_missing_dir() {
        let (temp_dir, installation) = setup();
        let path = temp_dir.keep();

        let mut missing_dir = path.clone();
        missing_dir.push("missing_dir/");
        let components = vec![missing_dir];

        let entries_to_delete = installation.get_paths_to_delete(path.as_path(), &components).unwrap();

        assert_eq!(true, entries_to_delete.is_empty());
    }

    #[test]
    fn test_one_needless_dir() {
        let (temp_dir, installation) = setup();
        let path = temp_dir.keep();

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
        let path = temp_dir.keep();

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
        let path = temp_dir.keep();

        let mut needless_path: PathBuf = path.clone();
        needless_path.push("dir/needless.file");
        fs::create_dir_all(needless_path.parent().unwrap()).unwrap();
        fs::File::create(&needless_path).unwrap();

        let mut missing_file = path.clone();
        missing_file.push("dir/missing.file");
        let components = vec![missing_file];

        let entries_to_delete = installation.get_paths_to_delete(path.as_path(), &components).unwrap();

        assert_eq!(false, entries_to_delete.is_empty());
        assert_entries_to_delete(&path, &vec![String::from("dir/needless.file")], &entries_to_delete);
    }

    #[test]
    fn test_one_needless_dir_in_subdir() {
        let (temp_dir, installation) = setup();
        let path = temp_dir.keep();

        let mut needless_path: PathBuf = path.clone();
        needless_path.push("dir/needless_dir");
        fs::create_dir_all(&needless_path).unwrap();

        let mut missing_dir = path.clone();
        missing_dir.push("dir/missing_dir");
        let components = vec![missing_dir];

        let entries_to_delete = installation.get_paths_to_delete(path.as_path(), &components).unwrap();

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

        let backup = installation.backup_path("lib/component.jar");
        fs::create_dir_all(backup.parent().unwrap()).unwrap();
        File::create(&backup).unwrap().write_all("old".as_bytes()).unwrap();

        let orig = installation.path("lib/component.jar");
        fs::create_dir_all(orig.parent().unwrap()).unwrap();
        File::create(&orig).unwrap().write_all("OK".as_bytes()).unwrap();

        installation.move_to_trash("lib/component.jar").unwrap();

        let components: Vec<ApplicationComponent> = vec!(ApplicationComponent {
            path: String::from("lib/component.jar"),
            url: String::from("http://host/file"),
            checksum: String::from(""),
            download_size: Some(50),
            size: 123,
            cache_path: None,
        });
        installation.restore_backup(&components);

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