use crate::descriptor::ApplicationArtifact;
use crate::download_manager::DownloadManager;
use crate::errors::*;
use crate::installation_manager::CheckResult::{NotOk, OkLocked};
use crate::installation_manager::InstallationManager;
use crate::{descriptor, jvm_starter, UserInterface};
use cluFlock::FlockLock;
use log::*;
use simplelog::*;
use std::fs::File;
use std::time::Instant;


pub struct JavaLauncher {

}

impl JavaLauncher {
    pub fn run(application_name: &'static str, application_descriptor_url: &str, public_key: Option<[u8; 32]>,
               ui: UserInterface) -> Result<()> {
        let start = Instant::now();
        let installation_manager = InstallationManager::new(application_name)?;

        let log_file = installation_manager.get_log_file()?;
        let mut builder = ConfigBuilder::new();
        let config = if builder.set_time_offset_to_local().is_ok() {
            builder.set_time_offset_to_local().unwrap().build()
        } else {
            builder.build()
        };
        CombinedLogger::init(
            vec![
                WriteLogger::new(LevelFilter::Debug, config, log_file)
            ]
        ).chain_err(|| ErrorKind::StorageError(format!("Could not create logger")))?;

        let download_manager = DownloadManager::new();

        debug!("Using application descriptor from {}", application_descriptor_url);
        let descriptor_content = download_manager.download_and_get(&application_descriptor_url)
            .or_else(|| installation_manager.get_descriptor())
            .chain_err(|| ErrorKind::DownloadError("Could not download application descriptor. Internet connection is required for first usage.".to_string()))?;

        installation_manager.store_descriptor(&descriptor_content)?;
        let descriptor = descriptor::ApplicationDescriptor::parse(&descriptor_content, public_key)?;

        let mut locked_files: Vec<Vec<FlockLock<File>>> = Vec::new();

        // download splash screen if required
        match installation_manager.check_artifact(descriptor.splash.clone()) {
            NotOk(splash) => {
                download_manager.download_and_store(&vec![splash], &installation_manager, &ui)?;
                match installation_manager.check_artifact(descriptor.splash.clone()) {
                    NotOk(_) => {
                        bail!("Could not download splash screen. Please try again. If the problem persist, please contact the application author");
                    }
                    OkLocked(files) => locked_files.push(files)
                }
            }
            OkLocked(files) => locked_files.push(files)
        }
        ui.show_splash(descriptor.version.clone(),
                       installation_manager.get_installation_root().to_path_buf().join(descriptor.splash.path.clone()));

        info!("Preparing {} version {}", descriptor.name, descriptor.version);
        installation_manager.restore_backup(&descriptor.artifacts);

        let mut files_to_download: Vec<ApplicationArtifact> = Vec::new();
        for check_result in installation_manager.check_artifacts(&descriptor.artifacts) {
            match check_result {
                NotOk(artifact) => files_to_download.push(artifact),
                OkLocked(files) => locked_files.push(files)
            }
        }
        download_manager.download_and_store(&files_to_download, &installation_manager, &ui)?;
        for result in installation_manager.check_artifacts(&files_to_download) {
            match result {
                NotOk(_) => {
                    bail!("Error during installation verification. Please try again. If the problem persist, please contact the application author");
                }
                OkLocked(files) => locked_files.push(files)
            }
        }
        installation_manager.create_unmanaged(&descriptor)?;
        installation_manager.delete_unused_files(&descriptor)?;

        let elapsed = start.elapsed();
        info!("Check finished in {} ms", elapsed.as_millis());

        info!("Starting {} version {}", descriptor.name, descriptor.version);
        jvm_starter::JvmStarter::start_jvm(&descriptor.jvm_params, &installation_manager.get_installation_root(), &ui)?;

        info!("Unlocking files");
        for f in locked_files {
            installation_manager.unlock_files(f)?;
        }

        return Ok(());
    }
}
