fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").map_or(false, |os| os == "windows") {
        println!("cargo:rerun-if-changed=assets/LlamaLift.ico");
        println!("cargo:rerun-if-changed=build.rs");

        match winres::WindowsResource::new()
            // Set the icon for the executable.
            .set_icon("assets/LlamaLift.ico")
            // Set version information
            .set("FileVersion", "0.1.1.0")
            .set("ProductVersion", "0.1.1")
            .set("ProductName", "LlamaLift")
            .set("FileDescription", "Ollama Model Management GUI")
            .set("LegalCopyright", "MIT License")
            .compile()
        {
            Ok(_) => { println!("Windows resources compiled successfully."); }
            Err(e) => { eprintln!("Error compiling Windows resources: {}", e); }
        }
    }
}
