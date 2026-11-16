use devhub_core::model::FirmwareFeed;

fn main() {
    // HTS Plus Pro の実フィード (初回 = 基準値、2 回目に偽の指紋 = 更新ありになるはず)。
    let feed = FirmwareFeed::MouseFit {
        ui_folder: "JM_052_3955".into(),
    };
    for last in ["", "fake-baseline"] {
        match devhub_update::check(&feed, None, last) {
            Ok(st) => println!(
                "mousefit last={last:?} → new={} ver={:?} fp={:?} url={:?}",
                st.new_available, st.version, st.fingerprint, st.url
            ),
            Err(e) => println!("mousefit last={last:?} → ERR {}", e.0),
        }
    }
    // SU66 (board_id=3145737) — 現在公開物なし → new=false のはず。
    let feed = FirmwareFeed::Sparklink {
        vid: 0x1CA6,
        pid: 0x3009,
        board_id: Some(3145737),
    };
    match devhub_update::check(&feed, Some("0.1.2.0"), "x") {
        Ok(st) => println!(
            "sparklink → new={} ver={:?} fp={:?}",
            st.new_available, st.version, st.fingerprint
        ),
        Err(e) => println!("sparklink → ERR {}", e.0),
    }
}
