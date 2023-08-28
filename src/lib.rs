#[macro_use]
extern crate error_chain;

use std::path::PathBuf;
use std::process;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::thread;

use error_chain::ChainedError;
use log::*;
use msgbox::IconType;

use java_launcher::JavaLauncher;
use ui::UserInterface;

use crate::ui::Message;

mod errors;
mod java_launcher;
mod ui;
mod validation;
mod descriptor;
mod download_manager;
mod installation_manager;
mod jvm_starter;

#[cfg(not(feature = "check-signature"))]
pub fn start(application_name: &'static str, application_descriptor_url: String) {
    start_internal(application_name, application_descriptor_url, None);
}

#[cfg(feature = "check-signature")]
pub fn start(application_name: &'static str, application_descriptor_url: String, application_public_key: [u8; 32]) {
    start_internal(application_name, application_descriptor_url, Some(application_public_key));
}

fn start_internal(application_name: &'static str, application_descriptor_url: String, application_public_key: Option<[u8; 32]>) {
    // create communication channel
    let (tx, rx) = mpsc::channel();
    let ui = UserInterface::new(tx);

    // start launcher in separate thread - this thread is reserved for UI stuff (required by macOS)
    thread::spawn(move || {
        let result = JavaLauncher::run(&application_name, &application_descriptor_url, application_public_key, ui.clone());
        match result {
            Ok(_) => {},
            Err(e) => {
                error!("{}", e.display_chain().to_string());
                ui.terminate(format!("{:}", e));
            }
        }
    });

    // wait until splash can be shown and provide an error message dialog functionality
    let (version, image_dir) = await_splash(&application_name, &rx);

    // show splash and download progress
    let mut splash = ui::splash::Splash::new(&application_name, version, image_dir);
    match splash.show_and_await_termination(rx) {
        Err(e) => {
            error!("{}", e.display_chain().to_string());
            show_error_message(&application_name, format!("{:}", e), true);
        },
        Ok(_) => ()
    };
}

pub fn show_error_message(application_name: &'static str, message: String, terminate: bool) {
    let title = String::from(application_name);
    match msgbox::create(&title, &message, IconType::Error) {
        Ok(()) => (),
        Err(_) => {
            error!("Could not show error message to user");
        }
    }
    if terminate {
        process::exit(1);
    }
}

fn await_splash(application_name: &'static str, rx: &Receiver<Message>) -> (String, PathBuf) {
    loop {
        match rx.recv() {
            Ok(Message::Error(val)) => {
                show_error_message(&application_name, val, true);
            },
            Err(e) => {
                error!("{}", e);
                show_error_message(&application_name, String::from(e.to_string()), true);
            },
            Ok(Message::SplashReady(version, image_dir)) => {
                return (version, image_dir);
            },
            Ok(_) => ()
        }
    }
}