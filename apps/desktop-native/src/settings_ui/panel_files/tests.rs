use super::*;

#[test]
fn private_key_size_is_bounded_before_decoding() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fixture.key");
    std::fs::write(&path, vec![b'x'; 16385]).unwrap();
    assert!(matches!(
        read_pem(&path, true),
        Err("panel.certificateSize")
    ));
    std::fs::write(&path, b"synthetic PEM").unwrap();
    let (_, key) = read_pem(&path, true).unwrap();
    assert_eq!(key.expose(), "synthetic PEM");
}

#[cfg(feature = "parity-render")]
#[gpui::test]
fn certificate_picker_cancel_and_late_reply_preserve_retirement(cx: &mut gpui::TestAppContext) {
    let (view, cx) = super::super::tests::view(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.preview("settings-server-certificate-open", w, cx);
            s.certificate_file = Some(("keep.pem".into(), "synthetic cert".into()));
            s.pick_certificate(true, w, cx);
        })
    });
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.files && !options.directories && !options.multiple);
        None
    });
    cx.run_until_parked();
    view.read_with(cx, |s, _| {
        assert!(!s.picking && s.private_key_file.is_none());
        assert_eq!(s.certificate_file.as_ref().unwrap().0, "keep.pem");
    });
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.pick_certificate(true, w, cx);
            s.clear(w, cx);
            s.show(3, false, w, cx);
        })
    });
    cx.simulate_path_prompt_response(|_| Some(vec!["/not/read/after/lock.key".into()]));
    cx.run_until_parked();
    view.read_with(cx, |s, _| {
        assert!(s.certificate_file.is_none() && s.private_key_file.is_none());
        assert!(!s.picking && s.error.is_empty());
    });
}

#[cfg(feature = "parity-render")]
#[gpui::test]
fn certificate_read_failure_can_retry_and_acknowledged_pair_is_retired(
    cx: &mut gpui::TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("fixture.key");
    std::fs::write(&key, vec![b'x'; 16385]).unwrap();
    let (view, cx) = super::super::tests::view(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.preview("settings-server-certificate-open", w, cx);
            s.certificate_file = Some(("fixture.pem".into(), "synthetic cert".into()));
            s.pick_certificate(true, w, cx);
        })
    });
    cx.update(|w, cx| w.draw(cx).clear(cx));
    cx.simulate_path_prompt_response(|_| Some(vec![key.clone()]));
    cx.run_until_parked();
    view.read_with(cx, |s, _| {
        assert_eq!(s.error, s.locale.text("panel.certificateSize"));
        assert!(!s.picking && !s.pending && s.private_key_file.is_none());
    });
    std::fs::write(&key, b"synthetic key").unwrap();
    cx.update(|w, cx| view.update(cx, |s, cx| s.pick_certificate(true, w, cx)));
    cx.simulate_path_prompt_response(|_| Some(vec![key]));
    cx.run_until_parked();
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            let request = s.take_request().expect("a complete pair is submitted");
            let Operation::Apply(operation) = request.operation else {
                panic!("panel request")
            };
            let AgentRequest::ControlPanelConfigure { certificate, .. } = *operation else {
                panic!("configure")
            };
            let certificate = certificate.unwrap();
            assert_eq!(certificate.private_key_pem.expose(), "synthetic key");
            assert_eq!(certificate.certificate_pem, "synthetic cert");
            assert!(s.error.is_empty());
            s.complete(
                Completion {
                    scope: request.scope,
                    result: Ok(Outcome::Panel(s.panel.clone().unwrap())),
                },
                w,
                cx,
            );
            assert!(s.certificate_file.is_none() && s.private_key_file.is_none());
            assert!(!s.pending);
        })
    });
}

#[cfg(feature = "parity-render")]
#[gpui::test]
fn certificate_export_cancel_failure_retry_and_retired_dialog(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("export.pem");
    let (view, cx) = super::super::tests::view(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.preview("settings-server-certificate-open", w, cx);
            s.export_certificate(w, cx);
        })
    });
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    view.read_with(cx, |s, _| assert!(!s.picking && s.error.is_empty()));
    cx.update(|w, cx| view.update(cx, |s, cx| s.export_certificate(w, cx)));
    cx.simulate_new_path_selection(|_| Some(dir.path().to_owned()));
    cx.run_until_parked();
    view.read_with(cx, |s, _| assert!(!s.picking && !s.error.is_empty()));
    cx.update(|w, cx| view.update(cx, |s, cx| s.export_certificate(w, cx)));
    cx.simulate_new_path_selection(|_| Some(target.clone()));
    cx.run_until_parked();
    view.read_with(cx, |s, _| {
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            *s.panel.as_ref().unwrap().certificate_pem.as_ref().unwrap()
        );
        assert!(!s.picking && s.error.is_empty());
    });
    std::fs::remove_file(&target).unwrap();
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.export_certificate(w, cx);
            s.select_tab(0, w, cx);
        })
    });
    cx.simulate_new_path_selection(|_| Some(target.clone()));
    cx.run_until_parked();
    assert!(!target.exists(), "a retired picker must not write");
}
