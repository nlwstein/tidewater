//! Game-file import (`docs/design/import.md`): finds the user's Warcraft II: Battle.net Edition files, checks them and
//! copies the ones Tidewater needs into its own data directory. No engine types, no SDL, no `unsafe`.
//!
//! - [`source`]: what kind of thing a path is (game folder, single archive, folder of archives, GOG installer, CD image, DOS copy).
//! - [`validate`]: opens the archives and checks the key files, with player-facing messages.
//! - [`copy`]: copies the allowlisted files into a temporary folder and swaps it into place.
//! - [`manifest`]: the record of what was copied (`manifest.json`).

#![forbid(unsafe_code)]

pub mod copy;
pub mod disc;
pub mod inno;
pub mod manifest;
pub mod source;
pub mod validate;

#[cfg(test)]
mod testiso;
#[cfg(test)]
mod testutil;

use std::path::{Path, PathBuf};

pub use copy::{Progress, Step, import, no_progress};
pub use inno::InstallerReader;
pub use manifest::Manifest;
pub use source::{Source, SourceKind, classify, classify_with};
pub use validate::{Issue, IssueKind, Report, Severity, validate};

/// The name of the folder under the data directory that holds the imported game files.
pub const GAME_DIR: &str = "game";

/// The imported game folder inside a data directory.
pub fn game_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(GAME_DIR)
}

/// Whether `data_dir` holds an imported game (its folder has `War2Dat.mpq`).
pub fn is_imported(data_dir: &Path) -> bool {
    use tidewater_formats::install::{Files, Folder};
    Folder::new(game_dir(data_dir))
        .open("War2Dat.mpq")
        .is_ok_and(|f| f.is_some())
}

/// Why an import (or a check) stopped. The text is written for the player and says what to do next.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("Couldn't find {}. Check the path and try again.", .0.display())]
    NotFound(PathBuf),
    #[error(
        "This looks like the 1995 DOS edition (it has .WAR files). Tidewater needs the Battle.net Edition, such as the \
         GOG release."
    )]
    DosEdition,
    #[error(
        "{} is an installer Tidewater can't read. Install the game with it, then choose the installed folder.",
        .0.display()
    )]
    InstallerUnsupported(PathBuf),
    #[error(
        "Reading {} needs the free innoextract program, which wasn't found. Install it (innoextract.constexpr.org; \
         `brew install innoextract` on macOS, your package manager on Linux) and try again, or install the game with \
         the installer and choose the installed folder.",
        .0.display()
    )]
    InnoextractMissing(PathBuf),
    #[error("Couldn't read the installer {}: {detail}. Install the game with it and choose the installed folder instead.", .path.display())]
    InstallerFailed { path: PathBuf, detail: String },
    #[error("Couldn't read the CD image {}: {detail}. Mount the image (or copy its files to a folder) and choose the mounted disc or the folder instead.", .path.display())]
    DiscImageUnreadable { path: PathBuf, detail: String },
    #[error(
        "{} is a CD image, but the game's files aren't stored on it as plain files: this disc holds an installer. \
         Tidewater can't read installers on discs yet. Install the game from the disc, then choose the installed \
         folder.",
        .0.display()
    )]
    DiscHoldsInstaller(PathBuf),
    #[error(
        "No Warcraft II game files (War2Dat.mpq) found in {}. Choose the folder where the game is installed.",
        .0.display()
    )]
    NoGameFiles(PathBuf),
    /// The files were found but are unusable; the issue says how.
    #[error("{}", .0.message)]
    Invalid(Issue),
    #[error("Not enough free space: about {} needed. Free some space and try again.", size_text(*needed))]
    NotEnoughSpace { needed: u64 },
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
}

impl ImportError {
    /// The process exit code for this kind of failure (0 is success; clap itself uses 2 for a bad command line).
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::NotFound(_) => 10,
            Self::DosEdition => 11,
            Self::InstallerUnsupported(_) | Self::InnoextractMissing(_) | Self::InstallerFailed { .. } => 12,
            Self::NoGameFiles(_) => 13,
            Self::DiscImageUnreadable { .. } | Self::DiscHoldsInstaller(_) => 19,
            Self::Invalid(issue) => match issue.kind {
                IssueKind::NoWar2Dat => 14,
                IssueKind::Damaged => 15,
                _ => 16,
            },
            Self::NotEnoughSpace { .. } => 17,
            Self::Io { .. } => 18,
        }
    }

    pub(crate) fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        if source.kind() == std::io::ErrorKind::StorageFull {
            return Self::NotEnoughSpace { needed: 0 };
        }
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

/// "412 MB", "3.5 GB".
pub fn size_text(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    if bytes == 0 {
        "some space".to_string()
    } else if bytes < MB {
        format!("{} KB", bytes.div_ceil(1024))
    } else if bytes < 1024 * MB {
        format!("{} MB", bytes.div_ceil(MB))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * MB as f64))
    }
}

/// The classification and the validation report as text, for `--import` and `twtool import-check`.
pub fn render_report(source: &Source, report: &Report) -> String {
    let mut s = String::new();
    s += &format!("Source: {} ({})\n", source.given.display(), source.kind.describe());
    if source.root != source.given && source.staged.is_none() {
        s += &format!("Game files found in: {}\n", source.root.display());
    }
    s += &format!("Edition: {}\n", report.edition);
    for a in &report.archives {
        s += &format!("  {}: {} files, {}\n", a.name, a.files, size_text(a.size));
    }
    match &source.maps {
        Some(_) => s += &format!("  Maps: {}\n", report.map_count),
        None => s += "  Maps: none\n",
    }
    for i in &report.issues {
        let tag = match i.severity {
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        s += &format!("{tag}: {}\n", i.message);
    }
    s += if report.is_ok() {
        "Result: can be imported\n"
    } else {
        "Result: can NOT be imported\n"
    };
    s
}

/// The result of a finished import as text.
pub fn render_summary(manifest: &Manifest, report: &Report, dest: &Path) -> String {
    let total: u64 = manifest.files.iter().map(|f| f.size).sum();
    let extras = if report
        .archives
        .iter()
        .any(|a| a.name.eq_ignore_ascii_case("install.mpq"))
    {
        "yes"
    } else {
        "no"
    };
    format!(
        "Imported {} into {}.\n  {} files, {}. Videos and music: {extras}. Maps: {}.\n",
        manifest.edition,
        dest.display(),
        manifest.files.len(),
        size_text(total),
        report.map_count,
    )
}
