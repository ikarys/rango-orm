//! Every crate root forbids `unsafe`. (`rango-bench` is an unpublished benchmark harness with
//! no crate root of its own and is not checked.)

#[test]
fn every_crate_root_forbids_unsafe_code() {
    let ws = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for root in [
        "rango-orm/src/lib.rs",
        "rango-orm-core/src/lib.rs",
        "rango-orm-derive/src/lib.rs",
        "rango-orm-postgres/src/lib.rs",
        "rango-orm-sqlite/src/lib.rs",
        "rango-orm-mysql/src/lib.rs",
        "rango-orm-cli/src/main.rs",
        "rango-tests/src/lib.rs",
    ] {
        let src = std::fs::read_to_string(ws.join(root)).unwrap();
        assert!(src.contains("#![forbid(unsafe_code)]"), "{root}");
    }
}
