use std::fs;
use std::path::{Path, PathBuf};

fn collect_rust_files(root: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(root).unwrap();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn implementation_files_stay_within_500_lines() {
    let src_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rust_files(&src_root, &mut files);

    let offenders: Vec<_> = files
        .into_iter()
        .filter(|path| {
            !path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with("_tests.rs")
        })
        .filter_map(|path| {
            let line_count = fs::read_to_string(&path).ok()?.lines().count();
            (line_count > 500).then_some((path, line_count))
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "implementation files must stay at or below 500 lines: {:?}",
        offenders
    );
}
