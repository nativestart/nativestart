#![windows_subsystem = "windows"]

use std::env;

#[cfg(target_os = "windows")]
const OS: &str = "windows";
#[cfg(target_os = "macos")]
const OS: &str = "mac";
#[cfg(target_os = "linux")]
const OS: &str = "linux";

#[cfg(target_arch = "x86_64")]
const ARCH: &str = "x86_64";

#[cfg(target_arch = "aarch64")]
const ARCH: &str = "aarch64";

const APPLICATION_NAME: &str = "APPLICATION_NAME                                                ";
const APPLICATION_DESCRIPTOR_URL: &str = "APPLICATION_DESCRIPTOR_URL                                                                                                                                                                                                                                      ";
#[cfg(feature = "check-signature")]
const APPLICATION_PUBLIC_KEY: &str = "$REPLACE_APPLICATION_PUBLIC_KEY$";

fn main() {
    #[cfg(target_os="windows")]
    attach_parent_console();

    let application_name = APPLICATION_NAME.trim_end();
    let application_descriptor_url = resolve_url();

    #[cfg(feature = "check-signature")]
    nativestart::start(application_name, application_descriptor_url, APPLICATION_PUBLIC_KEY.as_bytes().try_into().unwrap());

    #[cfg(not(feature = "check-signature"))]
    nativestart::start(application_name, application_descriptor_url);
}

fn resolve_url() -> String {
    let mut in_placeholder = false;
    let mut placeholder = String::new();
    let mut url = String::new();

    for c in APPLICATION_DESCRIPTOR_URL.trim().chars() {
        if c == '{' {
            in_placeholder = true;
        } else if c == '}' {
            if placeholder == "OS" {
                url.push_str(&OS)
            } else if placeholder == "ARCH" {
                url.push_str(&ARCH)
            } else if placeholder == "VERSION" {
                url.push_str(env!("CARGO_PKG_VERSION"))
            } else if placeholder.starts_with("env.") {
                match env::var(&placeholder[4..]) {
                    Ok(var) => url.push_str(&var),
                    Err(_) => ()
                }
            }
            placeholder.truncate(0);
            in_placeholder = false;
        } else if in_placeholder {
            placeholder.push(c);
        } else {
            url.push(c);
        }
    }
    url
}

#[cfg(target_os="windows")]
fn attach_parent_console() {
    use windows::Win32::System::Console::*;
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}
