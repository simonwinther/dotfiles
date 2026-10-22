use desktop_ring::{
    audio::parse_devices,
    core::{Snapshot, menu},
    monitors::*,
};
fn screen(name: &str, model: &str, serial: &str, w: u32, h: u32, scale: f64) -> Monitor {
    Monitor {
        name: name.into(),
        make: "Maker".into(),
        model: model.into(),
        serial: serial.into(),
        width: w,
        height: h,
        refresh: 60.0,
        scale,
        modes: vec![format!("{w}x{h}@60.00Hz")],
        ..Default::default()
    }
}
fn role(m: &Monitor, x: i32) -> FixedMonitor {
    FixedMonitor {
        identity: Identity {
            make: m.make.clone(),
            model: m.model.clone(),
            serial: Some(m.serial.clone()),
            edid: None,
        },
        mode: format!("{}x{}@60", m.width, m.height),
        scale: 1.0,
        transform: 0,
        x,
        y: 0,
    }
}
#[test]
fn physical_identity_survives_connector_renaming_and_order() {
    let a = screen("DP-1", "External", "A", 1920, 1080, 1.0);
    let b = screen("eDP-1", "Panel", "B", 1920, 1080, 1.2);
    let key = setup_key(&[a.clone(), b.clone()]);
    let mut a2 = a.clone();
    a2.name = "HDMI-A-8".into();
    assert_eq!(key, setup_key(&[b, a2]));
    let mut other = a;
    other.serial = "C".into();
    assert_ne!(
        setup_key(&[other]),
        setup_key(&[screen("DP-1", "External", "A", 1920, 1080, 1.0)])
    );
}
#[test]
fn duplicate_serialless_models_are_ambiguous() {
    let a = screen("DP-1", "Same", "0x00000000", 1920, 1080, 1.0);
    let mut b = a.clone();
    b.name = "DP-2".into();
    assert!(setup_key(&[a.clone(), b.clone()]).is_none());
    let p = FixedProfile {
        name: "ambiguous".into(),
        monitors: vec![
            FixedMonitor {
                identity: Identity {
                    make: a.make.clone(),
                    model: a.model.clone(),
                    ..Default::default()
                },
                ..role(&a, 0)
            },
            FixedMonitor {
                identity: Identity {
                    make: a.make.clone(),
                    model: a.model.clone(),
                    ..Default::default()
                },
                ..role(&a, 1920)
            },
        ],
    };
    assert!(match_profile(&p, &[a, b]).is_none());
}
#[test]
fn duplicate_models_with_unique_serials_match_roles_not_ports() {
    let a = screen("DP-1", "Same", "A", 1920, 1080, 1.0);
    let b = screen("DP-2", "Same", "B", 1920, 1080, 1.0);
    let p = FixedProfile {
        name: "desk".into(),
        monitors: vec![role(&a, 0), role(&b, 1920)],
    };
    assert_eq!(match_profile(&p, &[b, a]), Some(vec![1, 0]));
}
#[test]
fn edid_distinguishes_serialless_displays() {
    let mut a = screen("DP-1", "Same", "", 1920, 1080, 1.0);
    a.edid = Some("hash-a".into());
    let mut b = a.clone();
    b.name = "DP-2".into();
    b.edid = Some("hash-b".into());
    assert!(setup_key(&[a.clone(), b.clone()]).is_some());
    let id = Identity {
        make: a.make.clone(),
        model: a.model.clone(),
        serial: None,
        edid: a.edid.clone(),
    };
    assert!(id.score(&a).is_some());
    assert!(id.score(&b).is_none());
}
#[test]
fn reused_port_does_not_match_different_hardware() {
    let a = screen("HDMI-A-1", "Home", "A", 1920, 1080, 1.0);
    let b = screen("HDMI-A-1", "University", "B", 2560, 1440, 1.0);
    let p = FixedProfile {
        name: "home".into(),
        monitors: vec![role(&a, 0)],
    };
    assert!(match_profile(&p, &[b]).is_none());
}
#[test]
fn unsupported_mode_does_not_apply() {
    let a = screen("DP-1", "A", "A", 1920, 1080, 1.0);
    let mut r = role(&a, 0);
    r.mode = "1920x1080@240".into();
    assert!(
        fixed_plan(
            &[FixedProfile {
                name: "bad".into(),
                monitors: vec![r]
            }],
            &[a]
        )
        .is_err()
    );
}
#[test]
fn tied_profiles_are_rejected() {
    let a = screen("DP-1", "A", "A", 1920, 1080, 1.0);
    let p = FixedProfile {
        name: "one".into(),
        monitors: vec![role(&a, 0)],
    };
    assert!(fixed_plan(&[p.clone(), p], &[a]).is_err());
}
#[test]
fn rotation_and_fractional_scale_change_logical_extents() {
    assert_eq!(logical_size("1920x1080@144", 1.0, 3).unwrap(), (1080, 1920));
    assert_eq!(logical_size("1920x1080@60", 1.2, 0).unwrap(), (1600, 900));
    assert_eq!(logical_size("3840x2160@60", 2.0, 1).unwrap(), (1080, 1920));
    assert!(logical_size("1920x1080@60", 1.3, 0).is_err());
    assert!(logical_size("1920x1080@60", f64::NAN, 0).is_err());
}
#[test]
fn laptop_layouts_have_touching_edges_and_centered_alignment() {
    let i = screen("eDP-1", "Panel", "I", 1920, 1080, 1.2);
    let e = screen("DP-1", "External", "E", 2560, 1440, 1.0);
    let ms = [i, e];
    let l = laptop_plan(&ms, Position::Left, 1.2).unwrap();
    assert_eq!((l[1].x, l[1].y, l[0].x, l[0].y), (0, 270, 1600, 0));
    let r = laptop_plan(&ms, Position::Right, 1.2).unwrap();
    assert_eq!((r[0].x, r[0].y, r[1].x, r[1].y), (0, 0, 2560, 270));
    let b = laptop_plan(&ms, Position::Below, 1.2).unwrap();
    assert_eq!((b[0].x, b[0].y, b[1].x, b[1].y), (0, 0, 480, 1440));
}
#[test]
fn portrait_external_and_taller_panel_are_aligned_without_negative_positions() {
    let i = screen("eDP-1", "Panel", "I", 1920, 1080, 1.2);
    let mut e = screen("DP-1", "External", "E", 1920, 1080, 1.0);
    e.transform = 3;
    let l = laptop_plan(&[i, e], Position::Right, 1.2).unwrap();
    assert_eq!((l[1].x, l[1].y), (1080, 510));
}
#[test]
fn missing_or_extra_panels_do_not_offer_laptop_positions() {
    let a = screen("DP-1", "A", "A", 1920, 1080, 1.0);
    assert!(laptop_plan(std::slice::from_ref(&a), Position::Left, 1.0).is_err());
    assert!(laptop_plan(&[a.clone(), a], Position::Left, 1.0).is_err());
}
#[test]
fn unknown_fallback_preserves_active_screens_and_enables_only_disabled_outputs() {
    let mut a = screen("DP-1", "A", "A", 1920, 1080, 1.0);
    a.x = 100;
    let mut b = screen("HDMI-A-1", "B", "B", 1920, 1080, 1.0);
    b.disabled = true;
    let plan = fallback_plan(&[a.clone(), b.clone()]);
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].x, 2020);
    assert_eq!(plan[0].output, b.name);
    assert!(validate_plan(&plan, &[a, b]).is_ok());
    assert!(validate_plan(&[], &[]).is_err());
}
#[test]
fn idempotent_layout_does_not_reapply() {
    let a = screen("DP-1", "A", "A", 1920, 1080, 1.0);
    let r = current_rule(&a);
    assert!(!needs_apply(&r, &a));
    let mut b = a;
    b.transform = 3;
    assert!(needs_apply(&r, &b));
}
#[test]
fn independent_setups_keep_independent_preferences() {
    use desktop_ring::profiles::Preferences;
    let mut p = Preferences::default();
    p.layouts.insert("home".into(), Position::Left);
    p.layouts.insert("university".into(), Position::Below);
    let restored: Preferences = serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
    assert_eq!(restored.layouts["home"], Position::Left);
    assert_eq!(restored.layouts["university"], Position::Below);
}
#[test]
fn fixed_desktop_has_no_layout_actions_but_keeps_universal_menu() {
    let mut s = Snapshot::default();
    s.displays.profile = "Desk".into();
    let m = menu::build(&s);
    assert_eq!(m.entries.len(), 5);
    assert!(
        m.entries[0]
            .submenu
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .all(|e| e.id != "laptop-position")
    );
}
#[test]
fn sleep_is_nested_and_unavailable_hardware_cannot_be_selected() {
    let mut s = Snapshot::default();
    s.power.available = true;
    let m = menu::build(&s);
    let system = m
        .entries
        .iter()
        .find(|e| e.id == "system")
        .unwrap()
        .submenu
        .as_ref()
        .unwrap();
    assert_eq!(system.entries[0].label, "Sleep");
    assert!(!system.entries[0].enabled);
    s.power.can_suspend = true;
    let m = menu::build(&s);
    let system = m
        .entries
        .iter()
        .find(|e| e.id == "system")
        .unwrap()
        .submenu
        .as_ref()
        .unwrap();
    assert!(system.entries[0].enabled && system.entries[0].close);
    assert!(matches!(
        system.entries[0].action,
        Some(desktop_ring::core::Action::Suspend)
    ));
}
#[test]
fn audio_identity_ignores_temporary_indices_and_filters_monitor_sources() {
    let a = serde_json::json!([{"index":17,"name":"usb-stable","description":"USB Mic","properties":{"device.serial":"serial-1","device.profile.name":"mic"},"volume":{"mono":{"value":32768}},"mute":true},{"index":18,"name":"sink.monitor","properties":{"device.class":"monitor"}}]);
    let d = parse_devices(&a, "usb-stable", true);
    assert_eq!(d.len(), 1);
    assert!(d[0].active && d[0].muted);
    assert_eq!(d[0].volume, 50);
    let mut b = a;
    b[0]["index"] = serde_json::json!(42);
    assert_eq!(
        d[0].identity,
        parse_devices(&b, "usb-stable", true)[0].identity
    );
}
#[test]
fn audio_unavailable_port_is_visible_and_disabled() {
    let v = serde_json::json!([{"name":"hdmi","active_port":"port0","ports":[{"name":"port0","availability":"not available"}]}]);
    assert!(!parse_devices(&v, "", false)[0].available);
}
#[test]
fn menu_pagination_preserves_all_entries() {
    let m = menu::Menu {
        id: "root".into(),
        label: "test".into(),
        entries: (0..30)
            .map(|i| menu::Entry::info(&i.to_string(), "device", "", "x"))
            .collect(),
    };
    let mut p = menu::paged(&m);
    let mut count = 0;
    loop {
        assert!(p.entries.len() <= 8);
        count += p.entries.iter().filter(|e| e.submenu.is_none()).count();
        if let Some(next) = p.entries.iter().find_map(|e| e.submenu.clone()) {
            p = next;
        } else {
            break;
        }
    }
    assert_eq!(count, 30);
}
#[test]
fn lua_ipc_never_generates_disable_or_untrusted_code() {
    let a = screen("DP-1", "A", "A", 1920, 1080, 1.0);
    let mut r = current_rule(&a);
    let code = desktop_ring::monitors::ipc::lua_rule(&r).unwrap();
    assert!(code.contains("disabled=false"));
    r.output = "DP-1\"}; os.execute('bad')".into();
    assert!(desktop_ring::monitors::ipc::lua_rule(&r).is_err());
}

#[test]
fn precise_physical_profile_beats_generic_model_profile() {
    let m = screen("DP-1", "Model", "real-serial", 1920, 1080, 1.0);
    let exact = FixedProfile {
        name: "physical".into(),
        monitors: vec![role(&m, 0)],
    };
    let mut generic = exact.clone();
    generic.name = "generic".into();
    generic.monitors[0].identity.serial = None;
    let selected = fixed_plan(&[generic, exact], &[m]).unwrap().unwrap();
    assert_eq!(selected.0, "physical");
}

#[test]
fn laptop_specific_modes_are_selected_by_physical_identity() {
    let i = screen("eDP-1", "Panel", "I", 1920, 1080, 1.2);
    let mut e = screen("DP-9", "Home", "H", 1920, 1080, 1.0);
    e.modes.push("1920x1080@100.00Hz".into());
    let preference = MonitorPreference {
        identity: Identity {
            make: e.make.clone(),
            model: e.model.clone(),
            serial: Some(e.serial.clone()),
            edid: None,
        },
        mode: "1920x1080@100".into(),
        scale: 1.0,
        transform: 0,
    };
    let rules = configured_laptop_plan(&[i, e], Position::Below, 1.2, &[preference]).unwrap();
    assert_eq!(rules[0].mode, "1920x1080@100");
    assert_eq!(rules[1].x, 160);
}
