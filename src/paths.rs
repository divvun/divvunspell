//! Platform-specific paths for locating installed spell-checkers.
//!
//! Provides functions to find spell-checker files in standard system locations
//! based on language tags.
#[cfg(target_os = "macos")]
use std::path::PathBuf;
#[cfg(target_os = "windows")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::path::PathBuf;

#[cfg(target_os = "macos")]
use language_tags::LanguageTag;
#[cfg(target_os = "windows")]
use language_tags::LanguageTag;
#[cfg(target_os = "linux")]
use language_tags::LanguageTag;

#[cfg(target_os = "macos")]
/// Find a spell-checker file for the given language tag on macOS.
///
/// Searches for `.bhfst` or `.zhfst` files matching the language tag in:
/// 1. User services directory (`~/Library/Services`)
/// 2. System services directory (`/Library/Services`)
///
/// # Arguments
///
/// * `tag` - BCP 47 language tag (e.g., "en-US", "se")
///
/// # Returns
///
/// The path to the spell-checker file if found, or `None` if not found.
pub fn find_speller_path(tag: LanguageTag) -> Option<PathBuf> {
    let tag = tag.to_string();
    let pattern = format!("{tag}.{{bhfst,zhfst}}");
    if let Ok(path) = pathos::macos::user::services_dir() {
        if let Some(walker) = globwalk::GlobWalkerBuilder::new(path, &pattern)
            .build()
            .ok()
        {
            if let Some(v) = walker.into_iter().filter_map(Result::ok).next() {
                return Some(v.path().to_path_buf());
            }
        }
    }

    globwalk::GlobWalkerBuilder::new(pathos::macos::system::services_dir(), &pattern)
        .build()
        .ok()?
        .into_iter()
        .filter_map(Result::ok)
        .next()
        .map(|v| v.path().to_path_buf())
}

#[cfg(windows)]
/// Find a spell-checker file for the given language tag on Windows.
///
/// Searches for `.bhfst` or `.zhfst` files matching the language tag in
/// `C:\Program Files\WinDivvun\spellers`.
///
/// # Arguments
///
/// * `tag` - BCP 47 language tag (e.g., "en-US", "se")
///
/// # Returns
///
/// The path to the spell-checker file if found, or `None` if not found.
pub fn find_speller_path(tag: LanguageTag) -> Option<PathBuf> {
    let tag = tag.to_string();
    let pattern = format!("{tag}.{{bhfst,zhfst}}");

    globwalk::GlobWalkerBuilder::new(r"C:\Program Files\WinDivvun\spellers", &pattern)
        .build()
        .ok()?
        .into_iter()
        .filter_map(Result::ok)
        .next()
        .map(|x| x.path().to_path_buf())
}

#[cfg(target_os = "linux")]
/// Find a spell-checker file for the given language tag on Linux.
///
/// # Arguments
///
/// * `tag` - BCP 47 language tag (e.g., "en-US", "se")
///
/// # Returns
///
/// Currently always returns `None` as no standard paths are defined for Linux.
pub fn find_speller_path(tag: LanguageTag) -> Option<PathBuf> {
    None
}
