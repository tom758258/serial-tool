//! Operator-facing offline guide. No Core transport or Worker involvement.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use clap::{Args, ValueEnum};

const EN_HTML: &[u8] = include_bytes!("../help/cli.html");
const ZH_HTML: &[u8] = include_bytes!("../help/cli.zh-TW.html");
const CSS: &[u8] = include_bytes!("../help/help.css");
const ASSETS: [(&str, &[u8]); 3] = [
    ("cli.html", EN_HTML),
    ("cli.zh-TW.html", ZH_HTML),
    ("help.css", CSS),
];

#[derive(Debug, Args)]
pub(crate) struct UserGuideArgs {
    #[arg(long, value_enum, default_value_t = Language::En)]
    lang: Language,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Language {
    #[value(name = "en")]
    En,
    #[value(name = "zh-TW")]
    ZhTw,
}

impl Language {
    fn filename(self) -> &'static str {
        match self {
            Self::En => "cli.html",
            Self::ZhTw => "cli.zh-TW.html",
        }
    }
}

impl UserGuideArgs {
    pub(crate) fn run(self) -> i32 {
        let base = match user_cache_base() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("Cannot find offline Help cache location: {error}");
                return 3;
            }
        };
        match open_with(&base, self.lang, webbrowser::open) {
            Ok(()) => {
                println!("Opened offline user guide in the default browser.");
                0
            }
            Err(error) => {
                eprintln!("Cannot open offline user guide: {error}");
                3
            }
        }
    }
}

fn user_cache_base() -> io::Result<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not set"))
    }
    #[cfg(not(windows))]
    {
        if let Some(path) = env::var_os("XDG_CACHE_HOME") {
            return Ok(PathBuf::from(path));
        }
        env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".cache"))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
    }
}

// Stable content-derived directory name; byte-for-byte checks still verify each cached file.
fn fingerprint() -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for (name, bytes) in ASSETS {
        for byte in name.as_bytes().iter().copied().chain([0]).chain(bytes.iter().copied()) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

fn ensure_bundle(base: &Path) -> io::Result<PathBuf> {
    let dir = base
        .join("Serial Tool")
        .join("help")
        .join("cli")
        .join(format!("{:016x}", fingerprint()));
    fs::create_dir_all(&dir)?;
    for (name, bytes) in ASSETS {
        let target = dir.join(name);
        if fs::read(&target).is_ok_and(|existing| existing == bytes) {
            continue;
        }

        // Publish a complete file instead of exposing a partially written HTML/CSS file.
        let temporary = dir.join(format!("{name}.{}.tmp", std::process::id()));
        fs::write(&temporary, bytes)?;
        if let Err(error) = fs::remove_file(&target) {
            if error.kind() != io::ErrorKind::NotFound {
                let _ = fs::remove_file(&temporary);
                return Err(error);
            }
        }
        if let Err(error) = fs::rename(&temporary, &target) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    }
    Ok(dir)
}

fn file_url(path: &Path) -> io::Result<String> {
    let absolute = path.canonicalize()?;
    let normalized = absolute
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Help path is not UTF-8"))?
        .replace('\\', "/");
    let mut url = if normalized.starts_with("//") {
        String::from("file:")
    } else if normalized.starts_with('/') {
        String::from("file://")
    } else {
        String::from("file:///")
    };
    for byte in normalized.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'-' | b'_' | b'.' | b'~') {
            url.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(url, "%{byte:02X}").expect("writing to String cannot fail");
        }
    }
    Ok(url)
}

fn open_with(
    base: &Path,
    lang: Language,
    browser: impl FnOnce(&str) -> io::Result<()>,
) -> io::Result<()> {
    let dir = ensure_bundle(base)?;
    browser(&file_url(&dir.join(lang.filename()))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_bundle_reuses_and_repairs_assets_and_reports_browser_failures() {
        let root = env::temp_dir().join(format!("serial-help-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = ensure_bundle(&root).unwrap();
        assert_eq!(fs::read(dir.join("cli.html")).unwrap(), EN_HTML);
        assert_eq!(fs::read(dir.join("cli.zh-TW.html")).unwrap(), ZH_HTML);
        assert_eq!(fs::read(dir.join("help.css")).unwrap(), CSS);

        fs::write(dir.join("cli.zh-TW.html"), b"corrupt").unwrap();
        open_with(&root, Language::ZhTw, |url| {
            assert!(url.starts_with("file:///"));
            assert!(url.contains("cli.zh-TW.html"));
            Ok(())
        })
        .unwrap();
        assert_eq!(fs::read(dir.join("cli.zh-TW.html")).unwrap(), ZH_HTML);
        assert!(open_with(&root, Language::En, |_| {
            Err(io::Error::other("browser is unavailable"))
        })
        .is_err());

        fs::remove_dir_all(root).unwrap();
    }
}
