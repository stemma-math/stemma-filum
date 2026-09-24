//! What the pages load besides themselves.

mod support;

use support::Harness;

#[tokio::test]
async fn the_instance_serves_its_own_faces() {
    let mut instance = Harness::new();

    let face = instance.get("/assets/fonts/iosevka-400.woff2").await;

    assert!(face.status.is_success());
    assert_eq!(face.content_type.as_deref(), Some("font/woff2"));
    assert!(!face.body.is_empty());
}

#[tokio::test]
async fn the_instance_serves_its_own_surface_texture() {
    let mut instance = Harness::new();

    let texture = instance.get("/assets/noise.svg").await;

    assert!(texture.status.is_success());
    assert_eq!(
        texture.content_type.as_deref(),
        Some("image/svg+xml; charset=utf-8")
    );
    assert!(texture.says("feTurbulence"));
}

#[tokio::test]
async fn a_face_the_instance_does_not_have_is_not_invented() {
    let mut instance = Harness::new();

    let face = instance.get("/assets/fonts/helvetica-400.woff2").await;

    assert_eq!(face.status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn nothing_a_page_loads_comes_from_anywhere_else() {
    let mut instance = Harness::new();

    let stylesheet = instance.get("/assets/stemma.css").await;

    assert!(stylesheet.status.is_success());

    for reference in stylesheet.body.split("url(").skip(1) {
        let address = reference
            .trim_start_matches('"')
            .split(['"', ')'])
            .next()
            .unwrap_or_default();
        let same_origin = address.starts_with("/assets/")
            || address.starts_with("data:")
            || address.starts_with('#')
            || !(address.starts_with('/') || address.contains(':'));

        assert!(same_origin, "the stylesheet sends a reader to {address}");
    }
}
