error_chain!{
    foreign_links {
        Io(::std::io::Error);
    }

    errors {
        InvalidJSON(msg: String) {
            description("invalid JSON")
            display("Could not parse JSON descriptor: {:}", msg)
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