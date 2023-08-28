use log::*;
use simplelog::*;

use crate::{descriptor, jvm_starter, UserInterface};
use crate::download_manager::DownloadManager;
use crate::errors::*;
use crate::installation_manager::InstallationManager;


pub struct JavaLauncher {

}

impl JavaLauncher {
    pub fn run(application_name: &'static str, application_descriptor_url: &str, public_key: Option<[u8; 32]>,
               ui: UserInterface) -> Result<()> {
        let installation_manager = InstallationManager::new(application_name)?;

        let log_file = installation_manager.get_log_file()?;
        CombinedLogger::init(
            vec![
                WriteLogger::new(LevelFilter::Debug, Config::default(), log_file)
            ]
        ).chain_err(|| ErrorKind::StorageError(format!("Could not create logger")))?;

        let download_manager = DownloadManager::new();

        debug!("Using application descriptor from {}", application_descriptor_url);
        let descriptor_content = download_manager.download_and_get(&application_descriptor_url)
            .or_else(|| installation_manager.get_descriptor())
            .chain_err(|| ErrorKind::DownloadError(format!("Could not download application descriptor. Internet connection is required for first usage.")))?;

        installation_manager.store_descriptor(&descriptor_content)?;
        let descriptor = descriptor::ApplicationDescriptor::parse(&descriptor_content, public_key)?;

        // download splash screen if required
        let splash_desc = vec![descriptor.splash.clone()];
        let splash_to_download = installation_manager.get_files_to_download(&splash_desc);
        download_manager.download_and_store(&splash_to_download, &installation_manager, &ui)?;

        ui.show_splash(descriptor.version.clone(),
                       installation_manager.get_installation_root().to_path_buf().join(descriptor.splash.path.clone()));

        info!("Downloading {} version {}", descriptor.name, descriptor.version);
        let files_to_download = installation_manager.get_files_to_download(&descriptor.artifacts);
        download_manager.download_and_store(&files_to_download, &installation_manager, &ui)?;

        installation_manager.delete_unused_files(&descriptor)?;

        info!("Locking installation files");
        let locked_files = installation_manager.lock_installation(&descriptor);

        info!("Checking installation files");
        if !installation_manager.verify_installation(&descriptor) {
            bail!("Error during installation verification. Please try again. If the problem persist, please contact the application author");
        }

        info!("Starting {} version {}", descriptor.name, descriptor.version);
        jvm_starter::JvmStarter::start_jvm(&descriptor.jvm_params, &installation_manager.get_installation_root(), &ui)?;

        info!("Unlocking files");
        installation_manager.unlock_files(locked_files?)?;

        return Ok(());
    }
}
