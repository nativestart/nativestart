#![windows_subsystem = "windows"]

#[cfg(target_os = "windows")]
const OS: &str = "windows";
#[cfg(target_os = "macos")]
const OS: &str = "mac";
#[cfg(target_os = "linux")]
const OS: &str = "linux";

const APPLICATION_NAME: &str = "APPLICATION_NAME                                                ";
const APPLICATION_DESCRIPTOR_URL: &str = "APPLICATION_DESCRIPTOR_URL                                                                                                                                                                                                                                      ";
#[cfg(feature = "check-signature")]
const APPLICATION_PUBLIC_KEY: [u8; 32] = [b'$', b'R', b'E', b'P', b'L', b'A', b'C', b'E', b'_', b'A', b'P', b'P', b'L', b'I', b'C', b'A', b'T', b'I', b'O', b'N', b'_', b'P', b'U', b'B', b'L', b'I', b'C', b'_', b'K', b'E', b'Y', b'$'];

fn main() {
    let application_name = APPLICATION_NAME.trim_end();
    let application_descriptor_url = String::from(APPLICATION_DESCRIPTOR_URL)
        .trim()
        .replace("${OS}", OS)
        .replace("${VERSION}", env!("CARGO_PKG_VERSION"));

    #[cfg(feature = "check-signature")]
    nativestart::start(application_name, application_descriptor_url, APPLICATION_PUBLIC_KEY);

    #[cfg(not(feature = "check-signature"))]
    nativestart::start(application_name, application_descriptor_url);
}
