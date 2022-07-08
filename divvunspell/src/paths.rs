use std::path::PathBuf;

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

    None
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
