//! Handling of system paths containing spell-checkers on different OS.
#[cfg(target_os = "macos")]
use std::path::PathBuf;
#[cfg(target_os = "windows")]
use std::path::PathBuf;

#[cfg(target_os = "macos")]
use language_tags::LanguageTag;
#[cfg(target_os = "windows")]
use language_tags::LanguageTag;

#[cfg(target_os = "macos")]
pub fn find_speller_path(tag: LanguageTag) -> Option<PathBuf> {
    let tag = tag.to_string();
    let pattern = format!("{tag}.{{bhfst,zhfst}}");
    if let Ok(path) = pathos::macos::user::services_dir() {
        match globwalk::GlobWalkerBuilder::new(path, &pattern)
            .build()
            .unwrap()
            .into_iter()
            .filter_map(Result::ok)
            .next()
        {
            Some(v) => return Some(v.path().to_path_buf()),
            None => {}
        }
    }

    globwalk::GlobWalkerBuilder::new(pathos::macos::system::services_dir(), &pattern)
        .build()
        .unwrap()
        .into_iter()
        .filter_map(Result::ok)
        .next()
        .map(|v| v.path().to_path_buf())
}

#[cfg(windows)]
pub fn find_speller_path(tag: LanguageTag) -> Option<PathBuf> {
    let tag = tag.to_string();
    let pattern = format!("{tag}.{{bhfst,zhfst}}");

    globwalk::GlobWalkerBuilder::new(r"C:\Program Files\WinDivvun\spellers", &pattern)
        .build()
        .unwrap()
        .into_iter()
        .filter_map(Result::ok)
        .next()
        .map(|x| x.path().to_path_buf())
}
