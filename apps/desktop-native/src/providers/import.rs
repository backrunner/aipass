use super::Form;
use aipass_desktop_native::catalog::draft::Draft;
use gpui::{Context, Window};

impl Form {
    pub fn import_draft(&mut self, draft: Draft, window: &mut Window, cx: &mut Context<Self>) {
        let values = [
            &draft.title,
            &draft.secret_label,
            draft.api_key.expose(),
            &draft.endpoint,
            &draft.model,
            &draft.tags,
            &draft.notes,
            &draft.domains,
            &draft.console,
            &draft.concurrency,
            &draft.add_fields.aliases,
            draft.add_fields.headers.expose(),
            &draft.add_fields.group,
            &draft.add_fields.rate,
            &draft.add_fields.currency,
            &draft.add_fields.unit_price,
        ];
        for (index, value) in values.iter().enumerate() {
            self.inputs[index].update(cx, |input, cx| {
                input.set_value((*value).to_owned(), window, cx)
            });
            if !value.is_empty() {
                self.visible.insert(index);
            }
        }
        self.notes.update(cx, |input, cx| {
            input.set_value(draft.notes.clone(), window, cx)
        });
        self.draft = draft;
        // Secret editors own the only displayed copy after the transfer.
        self.draft.api_key = Default::default();
        self.draft.add_fields.headers = Default::default();
        cx.notify();
    }
}

#[cfg(all(test, feature = "parity-render"))]
mod tests {
    use super::*;
    use aipass_desktop_native::catalog::Action;
    use gpui::AppContext;

    #[gpui::test]
    fn imported_form_is_editable_preserves_metadata_and_retires_secret_editors(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_base::init);
        let mut form = None;
        let (_, cx) = cx.add_window_view(|w, cx| {
            let view = cx.new(|cx| Form::new(None, false, false, w, cx));
            form = Some(view.clone());
            gpui_base::Root::new(view, w, cx)
        });
        let form = form.unwrap();
        cx.update(|w,cx| form.update(cx, |s,cx| {
            let link=aipass_desktop_native::provider_links::parse("aipass-provider://v1/add?title=Imported&apiKey=synthetic&accountIdentity=fixture&secretLabel=Production&tag=team&endpoint=https%3A%2F%2Frelay.invalid&headers=%5B%5B%22x-test%22%2C%22synthetic-header%22%5D%5D".into()).unwrap();
            s.import_draft(aipass_desktop_native::provider_links::into_draft(link).ok().unwrap(),w,cx);
            assert_eq!(s.inputs[0].read(cx).value(), "Imported");
            assert_eq!(s.inputs[2].read(cx).value(), "synthetic");
            assert!(s.inputs[2].read(cx).presentation().is_masked());
            assert!(s.draft.api_key.expose().is_empty());
            assert!(s.draft.add_fields.headers.expose().is_empty());
            assert!(s.visible.contains(&11));
            s.inputs[0].update(cx,|s,cx|s.set_value("Edited",w,cx));
            s.save(cx);
            let Action::Add(input)=s.take_action().unwrap() else { panic!("reviewed add") };
            assert_eq!(input.title,"Edited");
            assert_eq!(input.account_identity.as_deref(),Some("fixture"));
            assert_eq!(input.headers,[("x-test".into(),"synthetic-header".into())]);
            let key=s.inputs[2].clone(); let headers=s.inputs[11].clone();
            s.clear(w,cx);
            assert!(key.read(cx).value().is_empty());
            assert!(headers.read(cx).value().is_empty());
            assert!(s.draft.imported.account_identity.is_none());
        }));
    }
}
