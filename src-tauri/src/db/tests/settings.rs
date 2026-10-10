use crate::db::*;

#[test]
fn settings_roundtrip() {
    let db = Db::connect_in_memory().unwrap();
    assert_eq!(db.setting_get("missing").unwrap(), None);
    db.setting_set("k", "v1").unwrap();
    assert_eq!(db.setting_get("k").unwrap().as_deref(), Some("v1"));
    db.setting_set("k", "v2").unwrap();
    assert_eq!(db.setting_get("k").unwrap().as_deref(), Some("v2"));
}
