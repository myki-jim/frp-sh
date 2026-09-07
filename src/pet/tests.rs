use super::*;
#[test]
fn twenty_unique_ascii_looks_with_stable_footprints() {
    let mut seen = std::collections::HashSet::new();
    for id in 0..20 {
        let picture = artwork(id, Mood::Direct, 0, false);
        assert_eq!(picture.len(), 13);
        assert!(picture
            .iter()
            .all(|s| s.len() == 44 && s.is_ascii() && !s.contains('\x1b')));
        assert!(seen.insert(picture));
    }
    for body in art::BODIES {
        assert!(body.lines().all(|l| l.len() <= 44));
    }
}
#[test]
fn controls_persist_only_on_explicit_actions() {
    let mut selected = 0;
    let mut s = Settings::default();
    assert!(!edit(crossterm::event::KeyCode::Up, &mut selected, &mut s));
    assert_eq!(selected, 19);
    assert_eq!(s.look, 0);
    assert!(edit(
        crossterm::event::KeyCode::Enter,
        &mut selected,
        &mut s
    ));
    assert_eq!(s.look, 19);
    let encoded = toml::to_string(&s).unwrap();
    let decoded: Settings = toml::from_str(&encoded).unwrap();
    assert_eq!(decoded.look, 19);
}
#[test]
fn states_use_connection_evidence_and_debounce_quality() {
    let info = SessionInfo {
        room: "1234".into(),
        mode: "guest".into(),
        ..Default::default()
    };
    let view = RoomView::default();
    assert_eq!(observe(&info, &view, &[]), Mood::Connecting);
    let mut link = ("host".into(), "relay".into(), String::new(), 0, 0, 0, 0, 0);
    assert_eq!(observe(&info, &view, &[link.clone()]), Mood::Connecting);
    link.5 = 1;
    link.7 = 50_000;
    assert_eq!(observe(&info, &view, &[link.clone()]), Mood::Relay);
    link.7 = 300_000;
    assert_eq!(observe(&info, &view, &[link]), Mood::Unstable);
    let mut tracker = Tracker::default();
    assert_eq!(tracker.update(Mood::Relay, 0), Mood::Relay);
    assert_eq!(tracker.update(Mood::Unstable, 1), Mood::Relay);
    assert_eq!(tracker.update(Mood::Unstable, 51), Mood::Unstable);
    assert_eq!(tracker.update(Mood::Reconnecting, 52), Mood::Reconnecting);
}
#[test]
fn reduced_motion_and_small_windows_are_safe() {
    assert_eq!(
        artwork(0, Mood::Connecting, 0, true),
        artwork(0, Mood::Connecting, 23, true)
    );
    assert_ne!(
        artwork(0, Mood::Connecting, 0, false),
        artwork(0, Mood::Connecting, 5, false)
    );
    for (w, h) in [(20, 8), (80, 24), (104, 22), (140, 42)] {
        let s = Settings::default();
        let frame = wardrobe(w, h, 19, &s, 0, "");
        assert!(frame.spans.iter().all(|p| p.x < w
            && p.y < h
            && unicode_width::UnicodeWidthStr::width(p.text.as_str()) <= usize::from(w - p.x)));
        let mut dock = Frame {
            spans: vec![],
            pages: 1,
        };
        append(&mut dock, w, h, &s, Mood::Waiting, 0);
        if w < 64 || h < 22 {
            assert!(dock.spans.is_empty());
        } else if w >= 104 {
            assert!(dock.spans.iter().all(|p| p.x >= space(w, h, &s)));
        } else {
            assert!(dock.spans.iter().all(|p| p.y >= content_height(w, h, &s)));
        }
    }
}
