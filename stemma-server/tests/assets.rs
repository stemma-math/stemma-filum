//! What the pages load besides themselves.

mod support;

use support::Harness;

#[tokio::test]
async fn the_instance_serves_its_own_faces() {
    let mut instance = Harness::new();

    let face = instance.get("/assets/fonts/jetbrains-mono-400.woff2").await;

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

#[tokio::test]
async fn the_instance_serves_its_icons_and_manifest_at_the_root() {
    let mut instance = Harness::new();

    let icon = instance.get("/favicon.ico").await;
    assert!(icon.status.is_success());
    assert_eq!(icon.content_type.as_deref(), Some("image/x-icon"));

    let touch = instance.get("/apple-touch-icon.png").await;
    assert_eq!(touch.content_type.as_deref(), Some("image/png"));

    let manifest = instance.get("/site.webmanifest").await;
    assert_eq!(
        manifest.content_type.as_deref(),
        Some("application/manifest+json")
    );
    assert!(manifest.says("/icon-512.png"));
    assert!(manifest.says("#030e29"));

    let front = instance.get("/").await;
    assert!(front.says("href=\"/favicon.svg\""));
    assert!(front.says("href=\"/site.webmanifest\""));
}
