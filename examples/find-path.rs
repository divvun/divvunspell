fn main() {
    let tag_arg = match std::env::args().skip(1).next() {
        Some(v) => v,
        None => {
            eprintln!("No tag passed.");
            return;
        }
    };

    let tag = tag_arg.parse().expect("Invalid tag");

    match divvunspell::paths::find_speller_path(tag) {
        Some(v) => println!("Found: {}", v.display()),
        None => println!("Not found!"),
    }
}
