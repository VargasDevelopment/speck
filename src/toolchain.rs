use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Debug)]
pub struct BuildArtifacts {
    pub executable: PathBuf,
    pub llvm_ir: PathBuf,
    pub llvm_bitcode: PathBuf,
    pub size: u64,
}

pub fn build(source_path: &Path, llvm_ir: &str) -> Result<BuildArtifacts, String> {
    let stem = source_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| "source file must have a valid UTF-8 file name".to_owned())?;
    let artifact_name: String = stem
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if artifact_name.is_empty() {
        return Err("source file name does not contain a usable executable name".into());
    }

    let build_dir = std::env::current_dir()
        .map_err(|error| format!("could not find current directory: {error}"))?
        .join("build");
    fs::create_dir_all(&build_dir)
        .map_err(|error| format!("could not create `{}`: {error}", build_dir.display()))?;

    let llvm_path = build_dir.join(format!("{artifact_name}.ll"));
    let bitcode_path = build_dir.join(format!("{artifact_name}.bc"));
    let object_path = build_dir.join(format!("{artifact_name}.o"));
    let executable_path = build_dir.join(&artifact_name);
    fs::write(&llvm_path, llvm_ir)
        .map_err(|error| format!("could not write `{}`: {error}", llvm_path.display()))?;

    run(
        "llvm-as",
        [
            llvm_path.as_os_str().to_owned(),
            OsString::from("-o"),
            bitcode_path.as_os_str().to_owned(),
        ],
    )?;
    run(
        "clang",
        [
            OsString::from("-O2"),
            OsString::from("-c"),
            bitcode_path.as_os_str().to_owned(),
            OsString::from("-o"),
            object_path.as_os_str().to_owned(),
        ],
    )?;

    let crumb_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("runtime/crumb");
    let mut crumb_objects = Vec::new();
    for source_name in ["crumb", "framebuffer", "present_ppm"] {
        let source = crumb_dir.join(format!("{source_name}.c"));
        let object = build_dir.join(format!("crumb_{source_name}.o"));
        run(
            "clang",
            [
                OsString::from("-std=c11"),
                OsString::from("-Os"),
                OsString::from("-ffunction-sections"),
                OsString::from("-fdata-sections"),
                OsString::from("-fno-asynchronous-unwind-tables"),
                OsString::from("-c"),
                source.as_os_str().to_owned(),
                OsString::from("-o"),
                object.as_os_str().to_owned(),
            ],
        )?;
        crumb_objects.push(object);
    }

    let mut link_args = vec![
        OsString::from("-fuse-ld=lld"),
        OsString::from("-Wl,--gc-sections,--strip-all"),
        object_path.as_os_str().to_owned(),
    ];
    link_args.extend(
        crumb_objects
            .iter()
            .map(|object| object.as_os_str().to_owned()),
    );
    link_args.push(OsString::from("-o"));
    link_args.push(executable_path.as_os_str().to_owned());
    run("clang", link_args)?;

    let size = fs::metadata(&executable_path)
        .map_err(|error| {
            format!(
                "could not inspect output `{}`: {error}",
                executable_path.display()
            )
        })?
        .len();
    Ok(BuildArtifacts {
        executable: executable_path,
        llvm_ir: llvm_path,
        llvm_bitcode: bitcode_path,
        size,
    })
}

fn run<I>(program: &str, args: I) -> Result<(), String>
where
    I: IntoIterator<Item = OsString>,
{
    let args: Vec<OsString> = args.into_iter().collect();
    let output = Command::new(program)
        .args(&args)
        .output()
        .map_err(|error| format!("could not run `{program}`: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_failure(program, &args, &output))
    }
}

fn command_failure(program: &str, args: &[OsString], output: &Output) -> String {
    let command = std::iter::once(program.to_owned())
        .chain(args.iter().map(|arg| arg.to_string_lossy().into_owned()))
        .collect::<Vec<_>>()
        .join(" ");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    format!(
        "toolchain command failed ({command})\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        stdout.trim_end(),
        stderr.trim_end()
    )
}
