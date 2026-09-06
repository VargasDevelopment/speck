use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// These inputs travel with the compiler that selected them, including headers
// and both supported hosts' presenters. They are never read from its build tree.
const FILES: &[(&str, &[u8])] = &[
    ("crumb.c", include_bytes!("../runtime/crumb/crumb.c")),
    ("crumb.h", include_bytes!("../runtime/crumb/crumb.h")),
    (
        "crumb_internal.h",
        include_bytes!("../runtime/crumb/crumb_internal.h"),
    ),
    ("input.c", include_bytes!("../runtime/crumb/input.c")),
    (
        "framebuffer.c",
        include_bytes!("../runtime/crumb/framebuffer.c"),
    ),
    (
        "present_ppm.c",
        include_bytes!("../runtime/crumb/present_ppm.c"),
    ),
    (
        "present_stream.c",
        include_bytes!("../runtime/crumb/present_stream.c"),
    ),
    (
        "present_cocoa.m",
        include_bytes!("../runtime/crumb/present_cocoa.m"),
    ),
    (
        "platform/posix_main.c",
        include_bytes!("../runtime/crumb/platform/posix_main.c"),
    ),
];

pub(crate) struct RuntimeSources {
    pub(crate) directory: PathBuf,
}

impl RuntimeSources {
    pub(crate) fn materialize(build_dir: &Path) -> Result<Self, String> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let sources = loop {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let directory = build_dir.join(format!(".crumb-sources-{}-{id}", std::process::id()));
            match fs::create_dir(&directory) {
                Ok(()) => break Self { directory },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!(
                        "could not create runtime source directory: {error}"
                    ));
                }
            }
        };
        for (name, contents) in FILES {
            let path = sources.directory.join(name);
            fs::create_dir_all(path.parent().expect("runtime file has a parent"))
                .and_then(|()| fs::write(&path, contents))
                .map_err(|error| {
                    format!("could not materialize runtime source `{name}`: {error}")
                })?;
        }
        Ok(sources)
    }
}

impl Drop for RuntimeSources {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
