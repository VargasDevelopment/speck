use std::collections::HashSet;
use std::fmt::Write;
use std::{env, fs, path::PathBuf};

const CATALOG: &str = "runtime/crumb/keys.def";
const VIEWER: &str = "src/dev/viewer.html";
const BROWSER_CODES: &str = "/* SPECK_BROWSER_CODES */";

struct Key {
    suffix: String,
    variant: String,
    id: u8,
    browser_code: String,
    macos_code: u16,
    modifier_mask: u32,
}

fn main() {
    println!("cargo:rerun-if-changed={CATALOG}");
    println!("cargo:rerun-if-changed={VIEWER}");
    let keys = parse_catalog(&fs::read_to_string(CATALOG).expect("read keyboard catalog"));
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    fs::write(output.join("keyboard.rs"), rust_catalog(&keys))
        .expect("write Rust keyboard catalog");

    let viewer = fs::read_to_string(VIEWER).expect("read viewer template");
    assert_eq!(
        viewer.matches(BROWSER_CODES).count(),
        1,
        "viewer must contain exactly one browser key placeholder"
    );
    let codes = keys
        .iter()
        .map(|key| format!("{:?}", key.browser_code))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        output.join("viewer.html"),
        viewer.replace(BROWSER_CODES, &codes),
    )
    .expect("write viewer with keyboard catalog");
}

fn parse_catalog(source: &str) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut suffixes = HashSet::new();
    let mut variants = HashSet::new();
    let mut browser_codes = HashSet::new();
    let mut native_codes = HashSet::new();
    let mut modifier_masks = HashSet::new();
    for (line_index, line) in source.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let invalid = || format!("{CATALOG}:{}: invalid keyboard row", line_index + 1);
        let fields = line
            .strip_prefix("SPECK_KEY(")
            .and_then(|line| line.strip_suffix(')'))
            .unwrap_or_else(|| panic!("{}", invalid()))
            .split(',')
            .map(str::trim)
            .collect::<Vec<_>>();
        assert_eq!(fields.len(), 6, "{}", invalid());
        let [suffix, variant, id, browser_code, macos_code, modifier_mask] = fields.as_slice()
        else {
            unreachable!()
        };
        assert!(
            !suffix.is_empty()
                && suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'),
            "{}: invalid C suffix",
            invalid()
        );
        let identifier = |value: &str| {
            value.starts_with(|character: char| character.is_ascii_uppercase())
                && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
        };
        assert!(identifier(variant), "{}: invalid Rust variant", invalid());
        let browser_code = browser_code
            .strip_prefix('"')
            .and_then(|code| code.strip_suffix('"'))
            .filter(|code| identifier(code))
            .unwrap_or_else(|| panic!("{}: invalid browser code", invalid()));
        // Keep numbers decimal and unadorned so C and Rust cannot interpret them differently.
        let decimal = |value: &str| {
            assert!(
                value == "0"
                    || (!value.starts_with('0') && value.bytes().all(|byte| byte.is_ascii_digit())),
                "{}: expected decimal integer",
                invalid()
            );
            value
                .parse::<u32>()
                .unwrap_or_else(|_| panic!("{}: invalid integer", invalid()))
        };
        let id = u8::try_from(decimal(id)).expect("key ID must fit in one byte");
        assert_eq!(
            usize::from(id),
            keys.len(),
            "{}: IDs must be contiguous and ordered",
            invalid()
        );
        let macos_code =
            u16::try_from(decimal(macos_code)).expect("macOS code must fit in two bytes");
        let modifier_mask = decimal(modifier_mask);
        assert!(
            suffixes.insert(*suffix),
            "{}: duplicate C suffix",
            invalid()
        );
        assert!(
            variants.insert(*variant),
            "{}: duplicate Rust variant",
            invalid()
        );
        assert!(
            browser_codes.insert(browser_code),
            "{}: duplicate browser code",
            invalid()
        );
        assert!(
            macos_code == u16::MAX || native_codes.insert(macos_code),
            "{}: duplicate native code",
            invalid()
        );
        assert!(
            modifier_mask == 0
                || (modifier_mask.is_power_of_two()
                    && macos_code != u16::MAX
                    && modifier_masks.insert(modifier_mask)),
            "{}: modifiers require a distinct sided mask and native code",
            invalid()
        );
        keys.push(Key {
            suffix: (*suffix).to_owned(),
            variant: (*variant).to_owned(),
            id,
            browser_code: browser_code.to_owned(),
            macos_code,
            modifier_mask,
        });
    }
    assert!(!keys.is_empty(), "keyboard catalog must not be empty");
    keys
}

fn rust_catalog(keys: &[Key]) -> String {
    let mut rust = String::from(
        "// Generated from runtime/crumb/keys.def.\n\
         #[derive(Clone, Copy, Debug, Eq, PartialEq)]\n\
         #[repr(u8)]\n\
         pub enum Key {\n",
    );
    for key in keys {
        writeln!(rust, "    {} = {},", key.variant, key.id).unwrap();
    }
    rust.push_str("}\npub const KEYS: &[KeyInfo] = &[\n");
    for key in keys {
        let macos_code = if key.macos_code == u16::MAX {
            "None".to_owned()
        } else {
            format!("Some({})", key.macos_code)
        };
        writeln!(rust, "    KeyInfo {{ key: Key::{}, name: \"KEY_{}\", browser_code: {:?}, macos_code: {}, modifier_mask: {} }},",
            key.variant, key.suffix, key.browser_code, macos_code, key.modifier_mask).unwrap();
    }
    rust.push_str("];\n");
    rust
}
