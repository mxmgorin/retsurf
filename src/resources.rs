use std::path::PathBuf;

#[derive(rust_embed::Embed)]
#[folder = "resources/servo"]
pub struct ServoResources;

/// Static reader registered with Servo via the `inventory`-based resource API.
static RESOURCE_READER: ServoResources = ServoResources;

// Registers the embedded resource reader. Requires Servo's default
// `baked-in-resources` feature to be disabled so this is the only reader.
servo::submit_resource_reader!(&RESOURCE_READER);

impl servo::resources::ResourceReaderMethods for ServoResources {
    fn read(&self, file: servo::resources::Resource) -> Vec<u8> {
        let file = file.filename();
        let resource = ServoResources::get(file).unwrap();

        resource.data.to_vec()
    }

    fn sandbox_access_files_dirs(&self) -> Vec<PathBuf> {
        vec![]
    }

    fn sandbox_access_files(&self) -> Vec<PathBuf> {
        vec![]
    }
}
