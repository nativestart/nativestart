error_chain!{
    foreign_links {
        Io(::std::io::Error);
    }

    errors {
        InvalidDescriptor(msg: String) {
            description("invalid descriptor")
            display("Could not parse descriptor: {:}", msg)
        }
        SignatureError(msg: String) {
            description("signature error")
            display("Signature error: {:}", msg)
        }
        DownloadError(msg: String) {
            description("download error")
            display("Error while downloading application artifacts: {:}", msg)
        }
        StorageError(msg: String) {
            description("storage error")
            display("Error while storing application artifacts: {:}", msg)
        }
        ValidationError(msg: String) {
            description("validation error")
            display("Error while validating application artifacts: {:}", msg)
        }
        SplashError(msg: String) {
            description("splash error")
            display("Error while showing splash screen: {:}", msg)
        }
        JavaExecutionError(msg: String) {
            description("Java execution error")
            display("Error while executing Java: {:}", msg)
        }
    }
}