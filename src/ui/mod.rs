use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

pub mod splash;


pub enum Message {
    Error(String),
    SplashReady(String, PathBuf),
    Downloading(Arc<AtomicUsize>),
    FilesReady,
    ApplicationUiVisible,
    ApplicationTerminated,
}
pub const MAX_DOWNLOAD_PROGRESS: usize = 1000;

#[derive(Clone)]
pub struct UserInterface {
    tx: Sender<Message>,
    download_progress: Arc<AtomicUsize>,
}

impl UserInterface {
    const NOT_INITIALIZED: usize = MAX_DOWNLOAD_PROGRESS + 1;

    pub fn new(tx: Sender<Message>) -> UserInterface {
        return UserInterface {
            tx,
            download_progress : Arc::new(AtomicUsize::new(UserInterface::NOT_INITIALIZED)),
        };
    }

    pub fn terminate(&self, message: String) {
        self.tx.send(Message::Error(message)).unwrap();
    }

    pub fn show_splash(&self, version: String, image_dir: PathBuf) {
        self.tx.send(Message::SplashReady(version, image_dir)).unwrap();
    }

    pub fn set_download_progress(&self, progress: f64) {
        let old_progress = self.download_progress.load(Ordering::SeqCst);
        let new_progress = (progress * MAX_DOWNLOAD_PROGRESS as f64) as usize;

        if new_progress != old_progress {
            self.download_progress.store(new_progress, Ordering::SeqCst);
        }
        if old_progress == UserInterface::NOT_INITIALIZED {
            self.tx.send(Message::Downloading(self.download_progress.clone())).unwrap();
        }
    }

    pub fn download_done(&self) {
        self.tx.send(Message::FilesReady).unwrap();
        self.download_progress.store(UserInterface::NOT_INITIALIZED, Ordering::SeqCst);
    }

    pub fn application_visible(&self) {
        self.tx.send(Message::ApplicationUiVisible).unwrap();
    }

    pub fn application_terminated(&self) {
        self.tx.send(Message::ApplicationTerminated).unwrap();
    }
}
