const CONTAINERFILE: &str = include_str!("../../../config/Containerfile");

#[test]
fn image_precreates_writable_xdg_data_parent() {
    let user_setup = CONTAINERFILE
        .split_once("RUN useradd")
        .expect("Containerfile should create the dev user")
        .1
        .split_once("\n\n")
        .expect("dev user setup should be a separate build step")
        .0;

    assert!(user_setup.contains("mkdir -p"));
    assert!(user_setup.contains("/home/dev/.local/share"));
    assert!(user_setup.contains("chown -R dev:dev"));
    assert!(user_setup.contains("/home/dev"));
}
