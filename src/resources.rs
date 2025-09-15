use std::path::PathBuf;

#[derive(rust_embed::Embed)]
#[folder = "resources/servo"]
pub struct ServoResources;

impl ServoResources {
    pub fn init() {
        servo::resources::set(Box::new(ServoResources));
    }
}

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
