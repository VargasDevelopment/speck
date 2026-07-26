use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostTarget {
    LinuxX86_64,
    MacOsArm64,
}

impl HostTarget {
    pub fn detect() -> Result<Self, String> {
        Self::from_os_arch(env::consts::OS, env::consts::ARCH)
    }

    fn from_os_arch(os: &str, arch: &str) -> Result<Self, String> {
        match (os, arch) {
            ("linux", "x86_64") => Ok(Self::LinuxX86_64),
            ("macos", "aarch64") => Ok(Self::MacOsArm64),
            _ => Err(format!(
                "unsupported host platform `{os}/{arch}`; Speck host-native builds currently \
                 support Linux x86-64 and macOS ARM64, and do not cross-compile"
            )),
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "Linux x86-64",
            Self::MacOsArm64 => "macOS ARM64",
        }
    }

    fn fallback_llvm_triple(self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "x86_64-unknown-linux-gnu",
            Self::MacOsArm64 => "arm64-apple-darwin",
        }
    }

    fn accepts_llvm_triple(self, triple: &str) -> bool {
        let triple = triple.to_ascii_lowercase();
        match self {
            Self::LinuxX86_64 => triple.starts_with("x86_64-") && triple.contains("linux"),
            Self::MacOsArm64 => {
                (triple.starts_with("arm64-") || triple.starts_with("aarch64-"))
                    && triple.contains("apple")
                    && (triple.contains("darwin") || triple.contains("macos"))
            }
        }
    }

    fn object_extension(self) -> &'static str {
        match self {
            Self::LinuxX86_64 | Self::MacOsArm64 => "o",
        }
    }

    fn executable_extension(self) -> &'static str {
        match self {
            Self::LinuxX86_64 | Self::MacOsArm64 => "",
        }
    }

    fn runtime_platform_sources(self) -> &'static [&'static str] {
        match self {
            Self::LinuxX86_64 | Self::MacOsArm64 => &["platform/posix_main.c"],
        }
    }

    fn c_compile_args(self) -> &'static [&'static str] {
        match self {
            Self::LinuxX86_64 => &[
                "-ffunction-sections",
                "-fdata-sections",
                "-fno-asynchronous-unwind-tables",
            ],
            Self::MacOsArm64 => &["-fno-asynchronous-unwind-tables"],
        }
    }

    fn link_args(self) -> &'static [&'static str] {
        match self {
            Self::LinuxX86_64 => &["-fuse-ld=lld", "-Wl,--gc-sections,--strip-all"],
            Self::MacOsArm64 => &["-Wl,-dead_strip,-S,-x"],
        }
    }

    fn missing_clang_help(self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "install Clang and ensure `clang` is on PATH",
            Self::MacOsArm64 => {
                "install Apple's Command Line Tools with `xcode-select --install`, or put Clang on PATH"
            }
        }
    }

    fn missing_linker_help(self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "install LLD and ensure `ld.lld` is on PATH",
            Self::MacOsArm64 => "install Apple's Command Line Tools with `xcode-select --install`",
        }
    }
}

impl fmt::Display for HostTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.description())
    }
}

#[derive(Debug)]
pub struct BuildEnvironment {
    target: HostTarget,
    clang: PathBuf,
    linker: PathBuf,
    llvm_as: Option<PathBuf>,
    sdk_path: Option<PathBuf>,
    llvm_target_triple: String,
}

impl BuildEnvironment {
    pub fn discover(target: HostTarget) -> Result<Self, String> {
        let clang = discover_clang(target)?;
        let linker = discover_linker(target)?;
        let sdk_path = discover_sdk(target)?;
        let llvm_as = discover_llvm_as(target);
        let llvm_target_triple = discover_llvm_triple(target, &clang)?;
        Ok(Self {
            target,
            clang,
            linker,
            llvm_as,
            sdk_path,
            llvm_target_triple,
        })
    }

    pub fn target(&self) -> HostTarget {
        self.target
    }

    pub fn clang(&self) -> &Path {
        &self.clang
    }

    pub fn linker(&self) -> &Path {
        &self.linker
    }

    pub fn llvm_target_triple(&self) -> &str {
        &self.llvm_target_triple
    }

    fn target_args(&self) -> Vec<OsString> {
        let mut args = vec![OsString::from(format!(
            "--target={}",
            self.llvm_target_triple
        ))];
        if let Some(sdk_path) = &self.sdk_path {
            args.push(OsString::from("-isysroot"));
            args.push(sdk_path.as_os_str().to_owned());
        }
        args
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LlvmValidation {
    LlvmAs(PathBuf),
    ClangDirect { reason: String },
}

impl fmt::Display for LlvmValidation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LlvmAs(path) => write!(formatter, "llvm-as ({})", path.display()),
            Self::ClangDirect { reason } => write!(formatter, "Clang direct ({reason})"),
        }
    }
}

#[derive(Debug)]
pub struct BuildArtifacts {
    pub executable: PathBuf,
    pub llvm_ir: PathBuf,
    pub llvm_bitcode: PathBuf,
    pub size: u64,
    pub llvm_validation: LlvmValidation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Presenter {
    Ppm,
    DevelopmentStream,
    Cocoa,
}

impl Presenter {
    const fn name(self) -> &'static str {
        match self {
            Self::Ppm => "ppm",
            Self::DevelopmentStream => "stream",
            Self::Cocoa => "cocoa",
        }
    }

    const fn source(self) -> &'static str {
        match self {
            Self::Ppm => "present_ppm.c",
            Self::DevelopmentStream => "present_stream.c",
            Self::Cocoa => "present_cocoa.m",
        }
    }

    const fn output_suffix(self) -> &'static str {
        match self {
            Self::Ppm => "",
            Self::DevelopmentStream => "_dev",
            Self::Cocoa => "_native",
        }
    }

    const fn compile_definitions(self) -> &'static [&'static str] {
        match self {
            Self::Ppm => &[],
            Self::DevelopmentStream => &["-DCRUMB_DEVELOPMENT=1", "-DCRUMB_PACED=1"],
            Self::Cocoa => &["-DCRUMB_COCOA=1", "-DCRUMB_PACED=1"],
        }
    }

    const fn source_compile_args(self) -> &'static [&'static str] {
        match self {
            Self::Ppm | Self::DevelopmentStream => &[],
            Self::Cocoa => &["-x", "objective-c"],
        }
    }

    const fn link_args(self) -> &'static [&'static str] {
        match self {
            Self::Ppm | Self::DevelopmentStream => &[],
            Self::Cocoa => &["-framework", "AppKit", "-framework", "CoreGraphics"],
        }
    }

    fn native_for_target(target: HostTarget) -> Result<Self, String> {
        match target {
            HostTarget::MacOsArm64 => Ok(Self::Cocoa),
            HostTarget::LinuxX86_64 => Err(format!(
                "native Cocoa presentation requires macOS ARM64; detected {target}. Use `speck \
                 build` for deterministic PPM output or `speck dev` for browser presentation on \
                 this host"
            )),
        }
    }
}

pub fn build(
    source_path: &Path,
    llvm_ir: &str,
    environment: &BuildEnvironment,
) -> Result<BuildArtifacts, String> {
    build_with_presenter(source_path, llvm_ir, environment, Presenter::Ppm)
}

pub fn build_for_development(
    source_path: &Path,
    llvm_ir: &str,
    environment: &BuildEnvironment,
) -> Result<BuildArtifacts, String> {
    build_with_presenter(
        source_path,
        llvm_ir,
        environment,
        Presenter::DevelopmentStream,
    )
}

pub fn build_for_native(
    source_path: &Path,
    llvm_ir: &str,
    environment: &BuildEnvironment,
) -> Result<BuildArtifacts, String> {
    let presenter = Presenter::native_for_target(environment.target())?;
    build_with_presenter(source_path, llvm_ir, environment, presenter)
}

fn build_with_presenter(
    source_path: &Path,
    llvm_ir: &str,
    environment: &BuildEnvironment,
    presenter: Presenter,
) -> Result<BuildArtifacts, String> {
    let artifact_name = artifact_name(source_path)?;
    let output_name = format!("{artifact_name}{}", presenter.output_suffix());
    let build_dir = env::current_dir()
        .map_err(|error| format!("could not find current directory: {error}"))?
        .join("build");
    fs::create_dir_all(&build_dir)
        .map_err(|error| format!("could not create `{}`: {error}", build_dir.display()))?;

    let llvm_path = build_dir.join(format!("{output_name}.ll"));
    let bitcode_path = build_dir.join(format!("{output_name}.bc"));
    let object_path = build_dir.join(format!(
        "{output_name}.{}",
        environment.target.object_extension()
    ));
    let executable_path = executable_path(&build_dir, &output_name, environment.target);
    fs::write(&llvm_path, llvm_ir)
        .map_err(|error| format!("could not write `{}`: {error}", llvm_path.display()))?;

    let llvm_validation =
        validate_and_compile_ir(environment, &llvm_path, &bitcode_path, &object_path)?;
    let crumb_objects = compile_runtime(environment, &build_dir, presenter)?;
    link_executable(
        environment,
        &object_path,
        &crumb_objects,
        &executable_path,
        presenter,
    )?;

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
        llvm_validation,
    })
}

fn artifact_name(source_path: &Path) -> Result<String, String> {
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
        Err("source file name does not contain a usable executable name".into())
    } else {
        Ok(artifact_name)
    }
}

fn executable_path(build_dir: &Path, artifact_name: &str, target: HostTarget) -> PathBuf {
    let extension = target.executable_extension();
    if extension.is_empty() {
        build_dir.join(artifact_name)
    } else {
        build_dir.join(format!("{artifact_name}.{extension}"))
    }
}

fn validate_and_compile_ir(
    environment: &BuildEnvironment,
    llvm_path: &Path,
    bitcode_path: &Path,
    object_path: &Path,
) -> Result<LlvmValidation, String> {
    if let Some(llvm_as) = &environment.llvm_as {
        let assemble_args = vec![
            llvm_path.as_os_str().to_owned(),
            OsString::from("-o"),
            bitcode_path.as_os_str().to_owned(),
        ];
        let assemble = command_output(llvm_as, &assemble_args)?;
        if assemble.status.success() {
            let mut compile_args = environment.target_args();
            compile_args.extend([
                OsString::from("-O2"),
                OsString::from("-c"),
                bitcode_path.as_os_str().to_owned(),
                OsString::from("-o"),
                object_path.as_os_str().to_owned(),
            ]);
            let compile = command_output(&environment.clang, &compile_args)?;
            if compile.status.success() {
                return Ok(LlvmValidation::LlvmAs(llvm_as.clone()));
            }
            return clang_direct_fallback(
                environment,
                llvm_path,
                bitcode_path,
                object_path,
                format!(
                    "{} produced bitcode incompatible with the selected Clang",
                    llvm_as.display()
                ),
                Some(command_failure(&environment.clang, &compile_args, &compile)),
            );
        }
        return clang_direct_fallback(
            environment,
            llvm_path,
            bitcode_path,
            object_path,
            format!("{} could not validate this IR", llvm_as.display()),
            Some(command_failure(llvm_as, &assemble_args, &assemble)),
        );
    }

    clang_direct_fallback(
        environment,
        llvm_path,
        bitcode_path,
        object_path,
        "standalone llvm-as was not found".into(),
        None,
    )
}

fn clang_direct_fallback(
    environment: &BuildEnvironment,
    llvm_path: &Path,
    bitcode_path: &Path,
    object_path: &Path,
    reason: String,
    earlier_failure: Option<String>,
) -> Result<LlvmValidation, String> {
    let mut validate_args = environment.target_args();
    validate_args.extend([
        OsString::from("-x"),
        OsString::from("ir"),
        OsString::from("-c"),
        OsString::from("-emit-llvm"),
        llvm_path.as_os_str().to_owned(),
        OsString::from("-o"),
        bitcode_path.as_os_str().to_owned(),
    ]);
    if let Err(clang_failure) = run(&environment.clang, validate_args) {
        return Err(combine_failures(earlier_failure, clang_failure));
    }

    let mut compile_args = environment.target_args();
    compile_args.extend([
        OsString::from("-x"),
        OsString::from("ir"),
        OsString::from("-O2"),
        OsString::from("-c"),
        llvm_path.as_os_str().to_owned(),
        OsString::from("-o"),
        object_path.as_os_str().to_owned(),
    ]);
    if let Err(clang_failure) = run(&environment.clang, compile_args) {
        return Err(combine_failures(earlier_failure, clang_failure));
    }

    Ok(LlvmValidation::ClangDirect { reason })
}

fn combine_failures(earlier: Option<String>, clang_failure: String) -> String {
    match earlier {
        Some(earlier) => format!(
            "standalone LLVM validation was unusable, and Clang's direct fallback also failed\n\n\
             first failure:\n{earlier}\n\nfallback failure:\n{clang_failure}"
        ),
        None => clang_failure,
    }
}

fn compile_runtime(
    environment: &BuildEnvironment,
    build_dir: &Path,
    presenter: Presenter,
) -> Result<Vec<PathBuf>, String> {
    let crumb_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("runtime/crumb");
    let common_sources = ["crumb.c", "framebuffer.c", presenter.source()];
    let sources = common_sources.into_iter().chain(
        environment
            .target
            .runtime_platform_sources()
            .iter()
            .copied(),
    );
    let mut objects = Vec::new();

    for source_name in sources {
        let source = crumb_dir.join(source_name);
        let object_stem = source_name
            .trim_end_matches(".c")
            .trim_end_matches(".m")
            .replace(['/', '\\'], "_");
        let object = build_dir.join(format!(
            "crumb_{}_{object_stem}.{}",
            presenter.name(),
            environment.target.object_extension()
        ));
        let mut args = environment.target_args();
        args.extend([
            OsString::from("-std=c11"),
            OsString::from("-Os"),
            OsString::from("-I"),
            crumb_dir.as_os_str().to_owned(),
        ]);
        args.extend(
            environment
                .target
                .c_compile_args()
                .iter()
                .map(OsString::from),
        );
        args.extend(presenter.compile_definitions().iter().map(OsString::from));
        if source_name == presenter.source() {
            args.extend(presenter.source_compile_args().iter().map(OsString::from));
        }
        args.extend([
            OsString::from("-c"),
            source.as_os_str().to_owned(),
            OsString::from("-o"),
            object.as_os_str().to_owned(),
        ]);
        run(&environment.clang, args)?;
        objects.push(object);
    }
    Ok(objects)
}

fn link_executable(
    environment: &BuildEnvironment,
    game_object: &Path,
    runtime_objects: &[PathBuf],
    executable: &Path,
    presenter: Presenter,
) -> Result<(), String> {
    let mut args = environment.target_args();
    args.extend(environment.target.link_args().iter().map(OsString::from));
    args.push(game_object.as_os_str().to_owned());
    args.extend(
        runtime_objects
            .iter()
            .map(|object| object.as_os_str().to_owned()),
    );
    args.extend(presenter.link_args().iter().map(OsString::from));
    args.push(OsString::from("-o"));
    args.push(executable.as_os_str().to_owned());
    run(&environment.clang, args)
}

fn discover_clang(target: HostTarget) -> Result<PathBuf, String> {
    let mut checked = Vec::new();
    if target == HostTarget::MacOsArm64
        && let Some(path) = probe_command_path("xcrun", &["--find", "clang"], &mut checked)
    {
        return Ok(path);
    }
    if let Some(path) = find_on_path("clang", &mut checked) {
        return Ok(path);
    }
    if target == HostTarget::MacOsArm64
        && let Some(path) = homebrew_llvm_tool("clang", &mut checked)
    {
        return Ok(path);
    }
    Err(missing_tool("Clang", &checked, target.missing_clang_help()))
}

fn discover_linker(target: HostTarget) -> Result<PathBuf, String> {
    let mut checked = Vec::new();
    let linker = match target {
        HostTarget::LinuxX86_64 => find_on_path("ld.lld", &mut checked),
        HostTarget::MacOsArm64 => probe_command_path("xcrun", &["--find", "ld"], &mut checked)
            .or_else(|| find_on_path("ld", &mut checked)),
    };
    linker.ok_or_else(|| missing_tool("native linker", &checked, target.missing_linker_help()))
}

fn discover_sdk(target: HostTarget) -> Result<Option<PathBuf>, String> {
    if target == HostTarget::LinuxX86_64 {
        return Ok(None);
    }
    let mut checked = Vec::new();
    probe_command_path("xcrun", &["--show-sdk-path"], &mut checked)
        .map(Some)
        .ok_or_else(|| {
            missing_tool(
                "macOS SDK",
                &checked,
                "install Apple's Command Line Tools with `xcode-select --install`",
            )
        })
}

fn discover_llvm_as(target: HostTarget) -> Option<PathBuf> {
    let mut checked = Vec::new();
    if let Some(path) = find_on_path("llvm-as", &mut checked) {
        return Some(path);
    }
    if target == HostTarget::MacOsArm64 {
        if let Some(path) = probe_command_path("xcrun", &["--find", "llvm-as"], &mut checked) {
            return Some(path);
        }
        return homebrew_llvm_tool("llvm-as", &mut checked);
    }
    None
}

fn discover_llvm_triple(target: HostTarget, clang: &Path) -> Result<String, String> {
    let args = [OsString::from("-dumpmachine")];
    let output = command_output(clang, &args)?;
    if !output.status.success() {
        return Err(command_failure(clang, &args, &output));
    }
    let triple = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if triple.is_empty() {
        return Ok(target.fallback_llvm_triple().into());
    }
    if !target.accepts_llvm_triple(&triple) {
        return Err(format!(
            "selected Clang `{}` reported target `{triple}`, which does not match the detected \
             host {target}; check which Clang is selected or rebuild Speck on a supported native host",
            clang.display()
        ));
    }
    Ok(triple)
}

fn find_on_path(name: &str, checked: &mut Vec<String>) -> Option<PathBuf> {
    let Some(path) = env::var_os("PATH") else {
        checked.push(format!("PATH lookup for `{name}` (PATH is unset)"));
        return None;
    };
    for directory in env::split_paths(&path) {
        let candidate = directory.join(name);
        checked.push(candidate.display().to_string());
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn probe_command_path(program: &str, args: &[&str], checked: &mut Vec<String>) -> Option<PathBuf> {
    let command = format!("{} {}", program, args.join(" "));
    match Command::new(program).args(args).output() {
        Ok(output) if output.status.success() => {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            let path = PathBuf::from(&value);
            checked.push(format!("{command} -> {value}"));
            path.exists().then_some(path)
        }
        Ok(output) => {
            checked.push(format!(
                "{command} (failed: {})",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
            None
        }
        Err(error) => {
            checked.push(format!("{command} (could not start: {error})"));
            None
        }
    }
}

fn homebrew_llvm_tool(name: &str, checked: &mut Vec<String>) -> Option<PathBuf> {
    let prefix = probe_command_path("brew", &["--prefix", "llvm"], checked)?;
    let candidate = prefix.join("bin").join(name);
    checked.push(candidate.display().to_string());
    candidate.is_file().then_some(candidate)
}

fn missing_tool(tool: &str, checked: &[String], resolution: &str) -> String {
    format!(
        "required tool not found: {tool}\ncandidates checked:\n  - {}\nresolution: {resolution}",
        checked.join("\n  - ")
    )
}

fn run<I, S>(program: &Path, args: I) -> Result<(), String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<OsString> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect();
    let output = command_output(program, &args)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_failure(program, &args, &output))
    }
}

fn command_output(program: &Path, args: &[OsString]) -> Result<Output, String> {
    Command::new(program).args(args).output().map_err(|error| {
        format!(
            "could not run required tool `{}`: {error}\ncommand: {}\nresolution: ensure the tool exists and is executable",
            program.display(),
            display_command(program, args)
        )
    })
}

fn command_failure(program: &Path, args: &[OsString], output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    format!(
        "toolchain command failed ({})\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        display_command(program, args),
        output.status,
        stdout.trim_end(),
        stderr.trim_end()
    )
}

fn display_command(program: &Path, args: &[OsString]) -> String {
    std::iter::once(program.as_os_str())
        .chain(args.iter().map(OsString::as_os_str))
        .map(|part| part.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_supported_hosts_and_rejects_others() {
        assert_eq!(
            HostTarget::from_os_arch("linux", "x86_64"),
            Ok(HostTarget::LinuxX86_64)
        );
        assert_eq!(
            HostTarget::from_os_arch("macos", "aarch64"),
            Ok(HostTarget::MacOsArm64)
        );
        assert!(
            HostTarget::from_os_arch("windows", "x86_64")
                .unwrap_err()
                .contains("unsupported host platform")
        );
    }

    #[test]
    fn accepts_only_compatible_clang_triples() {
        assert!(HostTarget::LinuxX86_64.accepts_llvm_triple("x86_64-pc-linux-gnu"));
        assert!(!HostTarget::LinuxX86_64.accepts_llvm_triple("arm64-apple-darwin"));
        assert!(HostTarget::MacOsArm64.accepts_llvm_triple("arm64-apple-darwin25.5.0"));
        assert!(HostTarget::MacOsArm64.accepts_llvm_triple("aarch64-apple-macosx"));
        assert!(!HostTarget::MacOsArm64.accepts_llvm_triple("x86_64-apple-darwin"));
    }

    #[test]
    fn linux_command_policy_preserves_lld_and_elf_flags() {
        let target = HostTarget::LinuxX86_64;
        assert_eq!(target.object_extension(), "o");
        assert_eq!(target.executable_extension(), "");
        assert_eq!(
            target.link_args(),
            ["-fuse-ld=lld", "-Wl,--gc-sections,--strip-all"]
        );
        assert!(target.c_compile_args().contains(&"-ffunction-sections"));
        assert_eq!(target.runtime_platform_sources(), ["platform/posix_main.c"]);
    }

    #[test]
    fn macos_command_policy_uses_apple_linker_flags() {
        let target = HostTarget::MacOsArm64;
        assert_eq!(target.object_extension(), "o");
        assert_eq!(target.executable_extension(), "");
        assert_eq!(target.link_args(), ["-Wl,-dead_strip,-S,-x"]);
        assert!(!target.link_args().iter().any(|arg| arg.contains("lld")));
        assert_eq!(target.runtime_platform_sources(), ["platform/posix_main.c"]);
    }

    #[test]
    fn presenter_selection_keeps_stream_code_out_of_normal_builds() {
        assert_eq!(Presenter::Ppm.source(), "present_ppm.c");
        assert_eq!(Presenter::DevelopmentStream.source(), "present_stream.c");
        assert_ne!(Presenter::Ppm.name(), Presenter::DevelopmentStream.name());
        assert!(Presenter::Ppm.compile_definitions().is_empty());
        assert!(Presenter::Ppm.link_args().is_empty());
    }

    #[test]
    fn cocoa_presenter_is_selected_only_for_macos_arm64() {
        assert_eq!(
            Presenter::native_for_target(HostTarget::MacOsArm64),
            Ok(Presenter::Cocoa)
        );
        let error = Presenter::native_for_target(HostTarget::LinuxX86_64)
            .expect_err("Linux must not select Cocoa");
        assert!(error.contains("requires macOS ARM64"));
        assert!(error.contains("speck build"));
    }

    #[test]
    fn cocoa_policy_uses_objective_c_and_apple_frameworks() {
        assert_eq!(Presenter::Cocoa.source(), "present_cocoa.m");
        assert_eq!(Presenter::Cocoa.output_suffix(), "_native");
        assert_eq!(
            Presenter::Cocoa.source_compile_args(),
            ["-x", "objective-c"]
        );
        assert_eq!(
            Presenter::Cocoa.compile_definitions(),
            ["-DCRUMB_COCOA=1", "-DCRUMB_PACED=1"]
        );
        assert_eq!(
            Presenter::Cocoa.link_args(),
            ["-framework", "AppKit", "-framework", "CoreGraphics"]
        );
        assert!(!Presenter::Ppm.link_args().contains(&"AppKit"));
        assert!(!Presenter::DevelopmentStream.link_args().contains(&"AppKit"));
    }
}
